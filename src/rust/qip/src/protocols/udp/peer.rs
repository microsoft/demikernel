use super::packet::UdpPacket;
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
        Ok(())
    }

    pub fn send(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        payload: Vec<u8>,
    ) -> Future<'a, ()> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        self.rt.start_task(move || {
            let options = rt.options();
            let fut = arp.query(dest_ipv4_addr);
            let dest_link_addr = {
                let dest_link_addr;
                loop {
                    match fut.poll(rt.clock()) {
                        Ok(a) => {
                            dest_link_addr = a;
                            break;
                        }
                        Err(Fail::TryAgain {}) => {
                            yield None;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }

                dest_link_addr
            };

            let mut packet = UdpPacket::new(payload.len());
            packet.payload_mut().copy_from_slice(&payload);
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
            packet.ipv4_mut().write_header(ipv4::Header {
                protocol: ipv4::Protocol::Udp,
                src_addr: options.my_ipv4_addr,
                dest_addr: dest_ipv4_addr,
            })?;

            rt.emit_effect(Effect::Transmit(Rc::new(packet.into())));
            let x: Rc<Any> = Rc::new(());
            Ok(x)
        })
    }
}
