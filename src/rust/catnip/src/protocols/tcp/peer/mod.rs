mod connection;
mod isn_generator;

#[cfg(test)]
mod tests;

use super::{
    error::TcpError,
    segment::{TcpSegment, TcpSegmentDecoder, DEFAULT_MSS},
};
use crate::{
    prelude::*,
    protocols::{arp, ip, ipv4},
    r#async::{Async, WhenAny},
};
use connection::{TcpConnection, TcpConnectionId};
use isn_generator::IsnGenerator;
use rand::seq::SliceRandom;
use std::{
    any::Any,
    collections::{HashMap, VecDeque},
    convert::TryFrom,
    net::Ipv4Addr,
    num::Wrapping,
    rc::Rc,
    time::Instant,
};

pub struct TcpPeer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
    open_ports: HashMap<ip::Port, TcpConnectionId>,
    async_work: WhenAny<'a, ()>,
    connections: HashMap<TcpConnectionId, TcpConnection>,
    // todo: this should be shared state.
    available_private_ports: VecDeque<ip::Port>,
    isn_generator: IsnGenerator,
}

impl<'a> TcpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> TcpPeer<'a> {
        // initialize the pool of available private ports.
        let available_private_ports = {
            let mut ports = Vec::new();
            for i in ip::Port::first_private_port().into()..65535 {
                ports.push(ip::Port::try_from(i).unwrap());
            }
            let mut rng = rt.borrow_rng();
            ports.shuffle(&mut *rng);
            VecDeque::from(ports)
        };
        let isn_generator = IsnGenerator::new(&rt);

        TcpPeer {
            rt,
            arp,
            open_ports: HashMap::new(),
            async_work: WhenAny::new(),
            connections: HashMap::new(),
            available_private_ports,
            isn_generator,
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("TcpPeer::receive(...)");
        let segment = TcpSegmentDecoder::try_from(datagram)?;
        let ipv4_header = segment.ipv4().header();
        let tcp_header = segment.header();
        // i haven't yet seen anything that explicitly disallows categories of
        // IP addresses but it seems sensible to drop datagrams where the
        // source address does not really support a connection.
        let src_ipv4_addr = ipv4_header.src_addr();
        if src_ipv4_addr.is_broadcast()
            || src_ipv4_addr.is_multicast()
            || src_ipv4_addr.is_unspecified()
        {
            return Err(Fail::Malformed {
                details: "only unicast addresses are supported by TCP",
            });
        }

        let dest_port = match tcp_header.dest_port() {
            Some(p) => p,
            None => {
                return Err(Fail::Malformed {
                    details: "destination port is zero",
                })
            }
        };

        debug!("dest_port => {:?}", dest_port);
        debug!("open_ports => {:?}", self.open_ports);
        if let Some(cxn) = self.open_ports.get(&dest_port) {
            if tcp_header.rst() {
                self.rt.emit_effect(Effect::TcpError(
                    TcpError::ConnectionRefused {},
                ));
                Ok(())
            } else {
                unimplemented!();
            }
        } else {
            let src_port = match tcp_header.src_port() {
                Some(p) => p,
                None => {
                    return Err(Fail::Malformed {
                        details: "source port is zero",
                    })
                }
            };

            let mut ack_num = tcp_header.seq_num()
                + Wrapping(u32::try_from(segment.text().len())?);
            // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/TCP_IP+Illustrated,+Volume+1:+The+Protocols/9780132808200/ch13.html#ch13):
            // > Although there is no data in the arriving segment, the SYN
            // > bit logically occupies 1 byte of sequence number space;
            // > therefore, in this example the ACK number in the reset
            // > segment is set to the ISN, plus the data length (0), plus 1
            // > for the SYN bit.
            if tcp_header.syn() {
                ack_num += Wrapping(1);
            }

            self.cast_segment(
                TcpSegment::default()
                    .dest_ipv4_addr(src_ipv4_addr)
                    .dest_port(src_port)
                    .src_port(dest_port)
                    .ack_num(ack_num)
                    .rst(),
            );
            Ok(())
        }
    }

    pub fn connect(
        &mut self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
    ) -> Result<()> {
        let options = self.rt.options();
        let src_port = self.acquire_private_port()?;
        let src_ipv4_addr = options.my_ipv4_addr;
        let cxn_id = TcpConnectionId {
            local: ipv4::Endpoint::new(options.my_ipv4_addr, src_port),
            remote: ipv4::Endpoint::new(dest_ipv4_addr, dest_port),
        };
        let isn = self.isn_generator.next(&cxn_id);
        let cxn = TcpConnection::new(cxn_id.clone());
        assert!(self.connections.insert(cxn_id.clone(), cxn).is_none());
        assert!(self.open_ports.insert(src_port, cxn_id).is_none());

        self.cast_segment(
            TcpSegment::default()
                .src_ipv4_addr(src_ipv4_addr)
                .src_port(src_port)
                .dest_ipv4_addr(dest_ipv4_addr)
                .dest_port(dest_port)
                .seq_num(isn)
                .mss(DEFAULT_MSS)
                .syn(),
        );
        Ok(())
    }

    fn cast_segment(&mut self, mut segment: TcpSegment) {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        let fut = self.rt.start_coroutine(move || {
            trace!("TcpPeer::cast_segment({:?})", segment,);
            let options = rt.options();
            segment.src_ipv4_addr = Some(options.my_ipv4_addr);
            segment.src_link_addr = Some(options.my_link_addr);
            let dest_link_addr =
                r#await!(arp.query(segment.dest_ipv4_addr.unwrap()), rt.now())
                    .unwrap();
            segment.dest_link_addr = Some(dest_link_addr);
            rt.emit_effect(Effect::Transmit(Rc::new(segment.encode())));

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        });

        self.async_work.add(fut);
    }

    fn acquire_private_port(&mut self) -> Result<ip::Port> {
        if let Some(p) = self.available_private_ports.pop_front() {
            Ok(p)
        } else {
            Err(Fail::ResourceExhausted {
                details: "no more private ports",
            })
        }
    }

    fn release_private_port(&mut self, port: ip::Port) {
        assert!(port.is_private());
        self.available_private_ports.push_back(port);
    }
}

impl<'a> Async<()> for TcpPeer<'a> {
    fn poll(&self, now: Instant) -> Option<Result<()>> {
        self.async_work.poll(now).map(|r| r.map(|_| ()))
    }
}
