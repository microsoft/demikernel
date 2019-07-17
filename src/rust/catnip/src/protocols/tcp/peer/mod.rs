mod isn_generator;
mod runtime;

#[cfg(test)]
mod tests;

use super::{
    connection::{TcpConnection, TcpConnectionHandle, TcpConnectionId},
    error::TcpError,
    segment::{TcpSegment, TcpSegmentDecoder, DEFAULT_MSS, MIN_MSS},
};
use crate::{
    prelude::*,
    protocols::{arp, ip, ipv4},
    r#async::{Async, WhenAny},
};
use isn_generator::IsnGenerator;
use rand::seq::SliceRandom;
use runtime::TcpRuntime;
use std::{
    any::Any,
    cell::RefCell,
    cmp::min,
    collections::{HashMap, HashSet, VecDeque},
    convert::TryFrom,
    num::Wrapping,
    rc::Rc,
    time::Instant,
};

pub struct TcpPeer<'a> {
    assigned_handles: HashMap<TcpConnectionHandle, TcpConnectionId>,
    async_work: WhenAny<'a, ()>,
    connections: Rc<RefCell<HashMap<TcpConnectionId, TcpConnection>>>,
    isn_generator: IsnGenerator,
    open_ports: HashSet<ip::Port>,
    passive_connections: HashMap<ipv4::Endpoint, TcpConnection>,
    rt: TcpRuntime<'a>,
    unassigned_connection_handles: VecDeque<TcpConnectionHandle>,
    unassigned_private_ports: VecDeque<ip::Port>, // todo: shared state.
}

impl<'a> TcpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> TcpPeer<'a> {
        // initialize the pool of available private ports.
        let unassigned_private_ports = {
            let mut ports = Vec::new();
            for i in ip::Port::first_private_port().into()..65535 {
                ports.push(ip::Port::try_from(i).unwrap());
            }
            let mut rng = rt.borrow_rng();
            ports.shuffle(&mut *rng);
            VecDeque::from(ports)
        };

        // initialize the pool of available connection handles.
        let unassigned_connection_handles = {
            let mut handles = Vec::new();
            for i in 1..u16::max_value() {
                handles.push(TcpConnectionHandle::new(i));
            }
            let mut rng = rt.borrow_rng();
            handles.shuffle(&mut *rng);
            VecDeque::from(handles)
        };

        let isn_generator = IsnGenerator::new(&rt);
        let rt = TcpRuntime::new(rt, arp);

        TcpPeer {
            assigned_handles: HashMap::new(),
            async_work: WhenAny::new(),
            connections: Rc::new(RefCell::new(HashMap::new())),
            isn_generator,
            open_ports: HashSet::new(),
            passive_connections: HashMap::new(),
            rt,
            unassigned_connection_handles,
            unassigned_private_ports,
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("TcpPeer::receive(...)");
        let decoder = TcpSegmentDecoder::try_from(datagram)?;
        let segment = TcpSegment::try_from(decoder)?;
        // i haven't yet seen anything that explicitly disallows categories of
        // IP addresses but it seems sensible to drop datagrams where the
        // source address does not really support a connection.
        let remote_ipv4_addr =
            segment.src_ipv4_addr.ok_or(Fail::Malformed {
                details: "source IPv4 address is missing",
            })?;
        if remote_ipv4_addr.is_broadcast()
            || remote_ipv4_addr.is_multicast()
            || remote_ipv4_addr.is_unspecified()
        {
            return Err(Fail::Malformed {
                details: "only unicast addresses are supported by TCP",
            });
        }

        let local_port = segment.dest_port.ok_or(Fail::Malformed {
            details: "destination port is zero",
        })?;

        debug!("local_port => {:?}", local_port);
        debug!("open_ports => {:?}", self.open_ports);
        if self.open_ports.contains(&local_port) {
            if segment.rst {
                self.rt.rt().emit_effect(Effect::TcpError(
                    TcpError::ConnectionRefused {},
                ));
                Ok(())
            } else {
                unimplemented!();
            }
        } else {
            let remote_port = segment.src_port.ok_or(Fail::Malformed {
                details: "source port is zero",
            })?;

            let mut ack_num = segment.seq_num
                + Wrapping(u32::try_from(segment.payload.len())?);
            // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/TCP_IP+Illustrated,+Volume+1:+The+Protocols/9780132808200/ch13.html#ch13):
            // > Although there is no data in the arriving segment, the SYN
            // > bit logically occupies 1 byte of sequence number space;
            // > therefore, in this example the ACK number in the reset
            // > segment is set to the ISN, plus the data length (0), plus 1
            // > for the SYN bit.
            if segment.syn {
                ack_num += Wrapping(1);
            }

            self.async_work.add(
                self.rt.cast(
                    TcpSegment::default()
                        .dest_ipv4_addr(remote_ipv4_addr)
                        .dest_port(remote_port)
                        .src_port(local_port)
                        .ack_num(ack_num)
                        .rst(),
                ),
            );
            Ok(())
        }
    }

    pub fn connect(
        &mut self,
        remote_endpoint: ipv4::Endpoint,
    ) -> Result<TcpConnectionHandle> {
        self.start_active_connection(remote_endpoint)
    }

    pub fn listen(&mut self, port: ip::Port) -> Result<()> {
        if self.open_ports.contains(&port) {
            return Err(Fail::ResourceBusy {
                details: "port already in use",
            });
        }

        assert!(self.open_ports.insert(port));
        Ok(())
    }

    pub fn start_active_connection(
        &mut self,
        remote_endpoint: ipv4::Endpoint,
    ) -> Result<TcpConnectionHandle> {
        let options = self.rt.rt().options();
        let cxnid = TcpConnectionId {
            local: ipv4::Endpoint::new(
                options.my_ipv4_addr,
                self.acquire_private_port()?,
            ),
            remote: remote_endpoint,
        };
        let handle = self.new_connection(cxnid.clone())?;

        let rt = self.rt.clone();
        let connections = self.connections.clone();
        self.async_work.add(self.rt.rt().start_coroutine(move || {
            let isn = {
                let connections = connections.borrow_mut();
                let cxn = connections.get(&cxnid).unwrap();
                cxn.seq_num()
            };

            r#await!(
                rt.cast(
                    TcpSegment::default()
                        .connection(&cxnid)
                        .seq_num(isn)
                        .mss(DEFAULT_MSS)
                        .syn()
                ),
                rt.rt().now()
            )?;

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        }));

        Ok(handle)
    }

    pub fn start_passive_connection(
        &mut self,
        local_port: ip::Port,
        syn_segment: TcpSegment,
    ) -> Result<()> {
        assert!(self.open_ports.contains(&local_port));
        assert!(syn_segment.syn);

        let options = self.rt.rt().options();
        let remote_ipv4_addr = syn_segment.src_ipv4_addr.unwrap();
        let remote_port = syn_segment.src_port.unwrap();
        let cxnid = TcpConnectionId {
            local: ipv4::Endpoint::new(options.my_ipv4_addr, local_port),
            remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
        };
        let _ = self.new_connection(cxnid.clone())?;
        let ack_num = syn_segment.seq_num + Wrapping(1);
        let mss = min(DEFAULT_MSS, syn_segment.mss.unwrap_or(MIN_MSS));

        let rt = self.rt.clone();
        let connections = self.connections.clone();
        self.async_work.add(self.rt.rt().start_coroutine(move || {
            let isn = {
                let connections = connections.borrow_mut();
                let cxn = connections.get(&cxnid).unwrap();
                cxn.seq_num()
            };

            r#await!(
                rt.cast(
                    TcpSegment::default()
                        .connection(&cxnid)
                        .seq_num(isn)
                        .ack_num(ack_num)
                        .mss(mss)
                        .syn()
                        .ack()
                ),
                rt.rt().now()
            )?;

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        }));

        Ok(())
    }

    fn acquire_private_port(&mut self) -> Result<ip::Port> {
        if let Some(p) = self.unassigned_private_ports.pop_front() {
            Ok(p)
        } else {
            Err(Fail::ResourceExhausted {
                details: "no more private ports",
            })
        }
    }

    fn release_private_port(&mut self, port: ip::Port) {
        assert!(port.is_private());
        self.unassigned_private_ports.push_back(port);
    }

    fn acquire_connection_handle(&mut self) -> Result<TcpConnectionHandle> {
        if let Some(h) = self.unassigned_connection_handles.pop_front() {
            Ok(h)
        } else {
            Err(Fail::ResourceExhausted {
                details: "no more connection handles",
            })
        }
    }

    fn release_connection_handle(&mut self, handle: TcpConnectionHandle) {
        self.unassigned_connection_handles.push_back(handle);
    }

    fn new_connection(
        &mut self,
        cxnid: TcpConnectionId,
    ) -> Result<TcpConnectionHandle> {
        let handle = self.acquire_connection_handle()?;
        let mut connections = self.connections.borrow_mut();
        let isn = self.isn_generator.next(&cxnid);
        let cxn = TcpConnection::new(cxnid.clone(), handle, isn);
        let local_port = cxnid.local.port();
        assert!(connections.insert(cxnid.clone(), cxn).is_none());
        assert!(self.assigned_handles.insert(handle, cxnid).is_none());
        self.open_ports.insert(local_port);
        Ok(handle)
    }
}

impl<'a> Async<()> for TcpPeer<'a> {
    fn poll(&self, now: Instant) -> Option<Result<()>> {
        self.async_work.poll(now).map(|r| r.map(|_| ()))
    }
}
