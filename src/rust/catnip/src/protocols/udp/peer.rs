use super::{header::UdpHeader, packet::UdpPacket};
use crate::{
    prelude::*,
    protocols::{arp, ethernet2, ipv4},
    r#async::Future,
};
use std::{any::Any, net::Ipv4Addr, rc::Rc};

pub struct UdpPeer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
}

impl<'a> UdpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> UdpPeer<'a> {
        UdpPeer { rt, arp }
    }

    pub fn receive(&mut self, packet: ipv4::Packet) -> Result<()> {
        trace!("UdpPeer::receive");
        let packet = UdpPacket::from(packet);
        let ipv4_header = packet.ipv4().read_header()?;
        assert_eq!(ipv4_header.protocol, ipv4::Protocol::Udp);
        let udp_header = packet.read_header()?;
        self.rt.emit_effect(Effect::Received {
            protocol: ipv4::Protocol::Udp,
            src_addr: ipv4_header.src_addr,
            src_port: udp_header.src_port,
            dest_port: udp_header.dest_port,
            payload: packet.payload().to_vec(),
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
        self.rt.start_task(move || {
            let options = rt.options();
            debug!("initiating ARP query");
            let fut = arp.query(dest_ipv4_addr);
            let dest_link_addr = {
                let dest_link_addr;
                loop {
                    debug!("great!");
                    let x = fut.poll(rt.clock());
                    debug!("o rly?!");
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
                            debug!("Not here!");
                            yield None;
                            debug!("back in action!");
                            continue;
                        }
                        Err(e) => {
                            debug!("o hai!");
                            return Err(e);
                        }
                    }
                }

                dest_link_addr
            };

            debug!("a");
            let mut packet = UdpPacket::new(payload.len());
            debug!("aa {} {}", payload.len(), packet.payload_mut().len());
            // the payload slice could end up being larger than what's
            // requested because of the minimum ethernet frame size, so we need
            // to trim what we get from `packet.payload_mut()` to make it the
            // same size as `payload`.
            packet.payload_mut()[..payload.len()].copy_from_slice(&payload);
            debug!("b");
            packet
                .ipv4_mut()
                .frame_mut()
                .write_header(ethernet2::Header {
                    dest_addr: dest_link_addr,
                    src_addr: options.my_link_addr,
                    // todo: there's got to be a way to automatically set this
                    // field.
                    ether_type: ethernet2::EtherType::Ipv4,
                })?;
            debug!("c");
            packet.ipv4_mut().write_header(ipv4::Header {
                protocol: ipv4::Protocol::Udp,
                src_addr: options.my_ipv4_addr,
                dest_addr: dest_ipv4_addr,
            })?;
            debug!("d");
            packet.write_header(UdpHeader {
                dest_port,
                src_port,
            })?;

            debug!("e");
            rt.emit_effect(Effect::Transmit(Rc::new(packet.into())));
            let x: Rc<Any> = Rc::new(());
            Ok(x)
        })
    }
}
