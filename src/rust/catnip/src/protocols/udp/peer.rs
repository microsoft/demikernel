// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::{UdpDatagram, UdpDatagramDecoder, UdpDatagramEncoder};
use crate::{
    prelude::*,
    protocols::{arp, icmpv4, ip, ipv4},
};
use fxhash::FxHashSet;
use std::{
    cell::RefCell, convert::TryFrom, net::Ipv4Addr, rc::Rc, time::Instant,
};
use std::future::Future;

pub struct UdpPeer {
    rt: Runtime,
    arp: arp::Peer,
    open_ports: FxHashSet<ip::Port>,
}

impl<'a> UdpPeer {
    pub fn new(rt: Runtime, arp: arp::Peer) -> UdpPeer {
        UdpPeer {
            rt,
            arp,
            open_ports: FxHashSet::default(),
        }
    }

    pub fn receive(
        &mut self,
        ipv4_datagram: ipv4::Datagram<'_>,
    ) -> Result<()> {
        trace!("UdpPeer::receive(...)");
        let decoder = UdpDatagramDecoder::try_from(ipv4_datagram)?;
        let udp_datagram = UdpDatagram::try_from(decoder)?;
        let dest_port = match udp_datagram.dest_port {
            Some(p) => p,
            None => {
                return Err(Fail::Malformed {
                    details: "destination port is zero",
                })
            }
        };

        if self.is_port_open(dest_port) {
            if udp_datagram.src_port.is_none() {
                return Err(Fail::Malformed {
                    details: "source port is zero",
                });
            }

            self.rt.emit_event(Event::UdpDatagramReceived(udp_datagram));
            Ok(())
        } else {
            // from [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch08.html):
            // > the source address cannot be a zero address, a loopback
            // > address, a broadcast address, or a multicast address
            let src_ipv4_addr = udp_datagram.src_ipv4_addr.unwrap();
            if src_ipv4_addr.is_broadcast()
                || src_ipv4_addr.is_loopback()
                || src_ipv4_addr.is_multicast()
                || src_ipv4_addr.is_unspecified()
            {
                return Err(Fail::Ignored {
                    details: "invalid IPv4 address type",
                });
            }

            self.send_icmpv4_error(src_ipv4_addr, decoder.as_bytes().to_vec());
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
    ) -> impl Future<Output=Result<()>> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        async move {
            let options = rt.options();
            debug!("initiating ARP query");
            let dest_link_addr = arp.query(dest_ipv4_addr).await.expect("XXX handle ARP failure");
            debug!(
                "ARP query complete ({} -> {})",
                dest_ipv4_addr, dest_link_addr
            );

            let mut bytes = UdpDatagramEncoder::new_vec(text.len());
            let mut encoder = UdpDatagramEncoder::attach(&mut bytes);
            // the text slice could end up being larger than what's
            // requested because of the minimum ethernet frame size, so we need
            // to trim what we get from `encoder.text()` to make it the same
            // size as `text`.
            encoder.text()[..text.len()].copy_from_slice(&text);
            let mut udp_header = encoder.header();
            udp_header.dest_port(dest_port);
            udp_header.src_port(src_port);
            let mut ipv4_header = encoder.ipv4().header();
            ipv4_header.src_addr(options.my_ipv4_addr);
            ipv4_header.dest_addr(dest_ipv4_addr);
            let mut frame_header = encoder.ipv4().frame().header();
            frame_header.dest_addr(dest_link_addr);
            frame_header.src_addr(options.my_link_addr);
            let _ = encoder.seal()?;
            rt.emit_event(Event::Transmit(Rc::new(RefCell::new(bytes))));
            Ok(())
        }
    }

    fn send_icmpv4_error(&mut self, dest_ipv4_addr: Ipv4Addr, datagram: Vec<u8>) {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        let fut = async move {
            trace!(
                "UdpPeer::send_icmpv4_error({:?}, {:?})",
                dest_ipv4_addr,
                datagram
            );
            let options = rt.options();
            debug!("initiating ARP query");
            let dest_link_addr = arp.query(dest_ipv4_addr).await.expect("XXX handle ARP failure");
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
            rt.emit_event(Event::Transmit(Rc::new(RefCell::new(bytes))));
            Ok::<_, Fail>(())
        };
        // XXX add this to a futures unordered.
        let _ = fut;
    }

    pub fn advance_clock(&self, _now: Instant) {
        unimplemented!()
    }
}
