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
    connections: HashMap<TcpConnectionId, Rc<RefCell<TcpConnection>>>,
    isn_generator: IsnGenerator,
    open_ports: HashSet<ip::Port>,
    passive_connections: HashMap<ipv4::Endpoint, TcpConnection>,
    tcp_rt: TcpRuntime<'a>,
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
        let tcp_rt = TcpRuntime::new(rt, arp);

        TcpPeer {
            assigned_handles: HashMap::new(),
            async_work: WhenAny::new(),
            connections: HashMap::new(),
            isn_generator,
            open_ports: HashSet::new(),
            passive_connections: HashMap::new(),
            tcp_rt,
            unassigned_connection_handles,
            unassigned_private_ports,
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("TcpPeer::receive(...)");
        let decoder = TcpSegmentDecoder::try_from(datagram)?;
        let segment = TcpSegment::try_from(decoder)?;
        let local_ipv4_addr = segment.dest_ipv4_addr.unwrap();
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

        let remote_port = segment.src_port.ok_or(Fail::Malformed {
            details: "source port is zero",
        })?;

        debug!("local_port => {:?}", local_port);
        debug!("open_ports => {:?}", self.open_ports);
        if self.open_ports.contains(&local_port) {
            if segment.rst {
                self.tcp_rt.rt().emit_effect(Effect::TcpError(
                    TcpError::ConnectionRefused {},
                ));
                return Ok(());
            }

            if segment.syn && !segment.ack {
                self.start_passive_connection(segment)?;
                return Ok(());
            }

            let cxnid = TcpConnectionId {
                local: ipv4::Endpoint::new(local_ipv4_addr, local_port),
                remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
            };

            let cxn = self.connections.get(&cxnid).unwrap();
            let mut cxn = cxn.borrow_mut();
            cxn.incoming_segments().push_front(segment);
            return Ok(());
        }

        // `local_port` is not open; send the appropriate RST segment.
        let mut ack_num =
            segment.seq_num + Wrapping(u32::try_from(segment.payload.len())?);
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
            self.tcp_rt.cast(
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
        let options = self.tcp_rt.rt().options();
        let cxnid = TcpConnectionId {
            local: ipv4::Endpoint::new(
                options.my_ipv4_addr,
                self.acquire_private_port()?,
            ),
            remote: remote_endpoint,
        };
        let cxn = self.new_connection(cxnid.clone())?;
        let handle = cxn.borrow().handle();

        let tcp_rt = self.tcp_rt.clone();
        self.async_work
            .add(self.tcp_rt.rt().start_coroutine(move || {
                let unit: Rc<dyn Any> = Rc::new(());
                let isn = cxn.borrow().seq_num();

                r#await!(
                    tcp_rt.cast(
                        TcpSegment::default()
                            .connection(&cxnid)
                            .seq_num(isn)
                            .mss(DEFAULT_MSS)
                            .syn()
                    ),
                    tcp_rt.rt().now()
                )?;

                yield_until!(
                    !cxn.borrow_mut().incoming_segments().is_empty(),
                    tcp_rt.rt().now()
                );

                let segment =
                    cxn.borrow_mut().incoming_segments().pop_front().unwrap();
                let ack_num = segment.seq_num + Wrapping(1);

                if !segment.syn || !segment.ack {
                    r#await!(
                        tcp_rt.cast(
                            TcpSegment::default()
                                .connection(&cxnid)
                                .ack_num(ack_num)
                                .rst()
                        ),
                        tcp_rt.rt().now()
                    )?;

                    return Ok(unit);
                }

                tcp_rt
                    .rt()
                    .emit_effect(Effect::TcpConnectionEstablished(handle));

                r#await!(
                    tcp_rt.cast(
                        TcpSegment::default()
                            .connection(&cxnid)
                            .seq_num(cxn.borrow_mut().incr_seq_num(1))
                            .ack_num(ack_num)
                            .ack()
                    ),
                    tcp_rt.rt().now()
                )?;

                Ok(unit)
            }));

        Ok(handle)
    }

    pub fn start_passive_connection(
        &mut self,
        syn_segment: TcpSegment,
    ) -> Result<()> {
        assert!(syn_segment.syn && !syn_segment.ack);
        let local_port = syn_segment.dest_port.unwrap();
        assert!(self.open_ports.contains(&local_port));

        let options = self.tcp_rt.rt().options();
        let remote_ipv4_addr = syn_segment.src_ipv4_addr.unwrap();
        let remote_port = syn_segment.src_port.unwrap();
        let cxnid = TcpConnectionId {
            local: ipv4::Endpoint::new(options.my_ipv4_addr, local_port),
            remote: ipv4::Endpoint::new(remote_ipv4_addr, remote_port),
        };
        let cxn = self.new_connection(cxnid.clone())?;
        let ack_num = syn_segment.seq_num + Wrapping(1);
        let mss = min(DEFAULT_MSS, syn_segment.mss.unwrap_or(MIN_MSS));

        let tcp_rt = self.tcp_rt.clone();
        self.async_work
            .add(self.tcp_rt.rt().start_coroutine(move || {
                let unit: Rc<dyn Any> = Rc::new(());
                let isn = cxn.borrow().seq_num();

                r#await!(
                    tcp_rt.cast(
                        TcpSegment::default()
                            .connection(&cxnid)
                            .seq_num(isn)
                            .ack_num(ack_num)
                            .mss(mss)
                            .syn()
                            .ack()
                    ),
                    tcp_rt.rt().now()
                )?;

                yield_until!(
                    !cxn.borrow_mut().incoming_segments().is_empty(),
                    tcp_rt.rt().now()
                );

                let segment =
                    cxn.borrow_mut().incoming_segments().pop_front().unwrap();
                // todo: how to validate `segment.ack_num`?
                if !segment.ack {
                    r#await!(
                        tcp_rt.cast(
                            TcpSegment::default().connection(&cxnid).rst()
                        ),
                        tcp_rt.rt().now()
                    )?;

                    return Ok(unit);
                }

                tcp_rt.rt().emit_effect(Effect::TcpConnectionEstablished(
                    cxn.borrow().handle(),
                ));

                Ok(unit)
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
    ) -> Result<Rc<RefCell<TcpConnection>>> {
        let handle = self.acquire_connection_handle()?;
        let isn = self.isn_generator.next(&cxnid);
        let cxn = Rc::new(RefCell::new(TcpConnection::new(
            cxnid.clone(),
            handle,
            isn,
        )));
        let local_port = cxnid.local.port();
        assert!(self
            .connections
            .insert(cxnid.clone(), cxn.clone())
            .is_none());
        assert!(self.assigned_handles.insert(handle, cxnid).is_none());
        self.open_ports.insert(local_port);
        Ok(cxn)
    }
}

impl<'a> Async<()> for TcpPeer<'a> {
    fn poll(&self, now: Instant) -> Option<Result<()>> {
        self.async_work.poll(now).map(|r| r.map(|_| ()))
    }
}
