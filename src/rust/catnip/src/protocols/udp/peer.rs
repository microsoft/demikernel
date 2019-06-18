use super::datagram::{UdpDatagram, UdpDatagramMut};
use crate::{
    prelude::*,
    protocols::{arp, ipv4},
    r#async::Future,
};
use std::{any::Any, convert::TryFrom, net::Ipv4Addr, rc::Rc};

pub struct UdpPeer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
}

impl<'a> UdpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> UdpPeer<'a> {
        UdpPeer { rt, arp }
    }

    pub fn receive(&mut self, datagram: ipv4::Datagram<'_>) -> Result<()> {
        trace!("UdpPeer::receive(...)");
        let datagram = UdpDatagram::try_from(datagram)?;
        let ipv4_header = datagram.ipv4().header();
        let udp_header = datagram.header();
        self.rt.emit_effect(Effect::Received {
            protocol: ipv4::Protocol::Udp,
            src_addr: ipv4_header.src_addr(),
            src_port: udp_header.src_port(),
            dest_port: udp_header.dest_port(),
            payload: datagram.payload().to_vec(),
        });

        Ok(())
    }

    pub fn cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: u16,
        src_port: u16,
        payload: Vec<u8>,
    ) -> Future<'a, ()> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        self.rt.start_coroutine(move || {
            let options = rt.options();
            debug!("initiating ARP query");
            let fut = arp.query(dest_ipv4_addr);
            let dest_link_addr = {
                let dest_link_addr;
                loop {
                    let x = fut.poll(rt.clock());
                    match x {
                        Ok(a) => {
                            debug!(
                                "ARP query complete ({} -> {})",
                                dest_ipv4_addr, a
                            );
                            dest_link_addr = a;
                            break;
                        }
                        Err(Fail::TryAgain {}) => {
                            yield None;
                            continue;
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }

                dest_link_addr
            };

            let mut bytes = super::new_datagram(payload.len());
            let mut datagram = UdpDatagramMut::from_bytes(&mut bytes)?;
            // the payload slice could end up being larger than what's
            // requested because of the minimum ethernet frame size, so we need
            // to trim what we get from `datagram.payload_mut()` to make it the
            // same size as `payload`.
            datagram.payload()[..payload.len()].copy_from_slice(&payload);
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

            rt.emit_effect(Effect::Transmit(Rc::new(bytes)));
            let x: Rc<Any> = Rc::new(());
            Ok(x)
        })
    }
}
