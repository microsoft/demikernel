use super::datagram::{UdpDatagram, UdpDatagramMut};
use crate::{
    prelude::*,
    protocols::{arp, icmpv4, ip, ipv4},
    r#async::{Async, Future, WhenAny},
};
use std::{
    any::Any, collections::HashSet, convert::TryFrom, net::Ipv4Addr, rc::Rc,
    time::Instant,
};

pub struct UdpPeer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
    open_ports: HashSet<ip::Port>,
    async_work: WhenAny<'a, ()>,
}

impl<'a> UdpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> UdpPeer<'a> {
        UdpPeer {
            rt,
            arp,
            open_ports: HashSet::new(),
            async_work: WhenAny::new(),
        }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("UdpPeer::receive(...)");
        let datagram = UdpDatagram::try_from(datagram)?;
        let ipv4_header = datagram.ipv4().header();
        let udp_header = datagram.header();
        let dest_port = match udp_header.dest_port() {
            Some(p) => p,
            None => {
                return Err(Fail::Malformed {
                    details: "destination port is zero",
                })
            }
        };

        if self.is_port_open(dest_port) {
            let src_port = match udp_header.src_port() {
                Some(p) => p,
                None => {
                    return Err(Fail::Malformed {
                        details: "source port is zero",
                    })
                }
            };

            self.rt.emit_event(Event::BytesReceived {
                protocol: ipv4::Protocol::Udp,
                src_addr: ipv4_header.src_addr(),
                src_port,
                dest_port,
                text: IoVec::from(datagram.text().to_vec()),
            });

            Ok(())
        } else {
            // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch08.html):
            // > the source address cannot be a zero address, a loopback
            // > address, a broadcast address, or a multicast address
            let src_ipv4_addr = ipv4_header.src_addr();
            if src_ipv4_addr.is_broadcast()
                || src_ipv4_addr.is_loopback()
                || src_ipv4_addr.is_multicast()
                || src_ipv4_addr.is_unspecified()
            {
                return Err(Fail::Ignored {});
            }

            self.send_icmpv4_error(
                src_ipv4_addr,
                datagram.as_bytes().to_vec(),
            );
            Ok(())
        }
    }

    pub fn is_port_open(&self, port: ip::Port) -> bool {
        self.open_ports.contains(&port)
    }

    pub fn open_port(&mut self, port: ip::Port) {
        assert!(self.open_ports.replace(port).is_none());
    }

    pub fn close_port(&mut self, port: ip::Port) {
        assert!(self.open_ports.remove(&port));
    }

    pub fn cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
        src_port: ip::Port,
        text: Vec<u8>,
    ) -> Future<'a, ()> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        self.rt.start_coroutine(move || {
            let options = rt.options();
            debug!("initiating ARP query");
            let dest_link_addr =
                r#await!(arp.query(dest_ipv4_addr), rt.now()).unwrap();
            debug!(
                "ARP query complete ({} -> {})",
                dest_ipv4_addr, dest_link_addr
            );

            let mut bytes = UdpDatagram::new_vec(text.len());
            let mut datagram = UdpDatagramMut::attach(&mut bytes);
            // the text slice could end up being larger than what's
            // requested because of the minimum ethernet frame size, so we need
            // to trim what we get from `datagram.text_mut()` to make it the
            // same size as `text`.
            datagram.text()[..text.len()].copy_from_slice(&text);
            let mut udp_header = datagram.header();
            udp_header.dest_port(dest_port);
            udp_header.src_port(src_port);
            let mut ipv4_header = datagram.ipv4().header();
            ipv4_header.src_addr(options.my_ipv4_addr);
            ipv4_header.dest_addr(dest_ipv4_addr);
            let mut frame_header = datagram.ipv4().frame().header();
            frame_header.dest_addr(dest_link_addr);
            frame_header.src_addr(options.my_link_addr);
            let _ = datagram.seal()?;
            rt.emit_event(Event::Transmit(Rc::new(bytes)));

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        })
    }

    fn send_icmpv4_error(
        &mut self,
        dest_ipv4_addr: Ipv4Addr,
        datagram: Vec<u8>,
    ) {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        let fut = self.rt.start_coroutine(move || {
            trace!(
                "UdpPeer::send_icmpv4_error({:?}, {:?})",
                dest_ipv4_addr,
                datagram
            );
            let options = rt.options();
            debug!("initiating ARP query");
            let dest_link_addr =
                r#await!(arp.query(dest_ipv4_addr), rt.now()).unwrap();
            debug!(
                "ARP query complete ({} -> {})",
                dest_ipv4_addr, dest_link_addr
            );

            // this datagram should have already been validated by the caller.
            let datagram =
                ipv4::Datagram::attach(datagram.as_slice()).unwrap();
            let mut bytes = icmpv4::Error::new_vec(datagram);
            let mut error = icmpv4::ErrorMut::attach(&mut bytes);
            error.id(icmpv4::ErrorId::DestinationUnreachable(
                icmpv4::DestinationUnreachable::DestinationPortUnreachable,
            ));
            let ipv4 = error.icmpv4().ipv4();
            let mut ipv4_header = ipv4.header();
            ipv4_header.src_addr(options.my_ipv4_addr);
            ipv4_header.dest_addr(dest_ipv4_addr);
            let frame = ipv4.frame();
            let mut frame_header = frame.header();
            frame_header.src_addr(options.my_link_addr);
            frame_header.dest_addr(dest_link_addr);
            let _ = error.seal()?;
            rt.emit_event(Event::Transmit(Rc::new(bytes)));

            let x: Rc<dyn Any> = Rc::new(());
            Ok(x)
        });

        self.async_work.add(fut);
    }
}

impl<'a> Async<()> for UdpPeer<'a> {
    fn poll(&self, now: Instant) -> Option<Result<()>> {
        self.async_work.poll(now).map(|r| r.map(|_| ()))
    }
}
