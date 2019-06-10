use crate::{
    prelude::*,
    protocols::{arp, ipv4},
    r#async::Future,
};
use std::net::Ipv4Addr;

pub struct UdpPeer<'a> {
    rt: Runtime<'a>,
    arp: arp::Peer<'a>,
}

impl<'a> UdpPeer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> UdpPeer<'a> {
        UdpPeer { rt, arp }
    }

    pub fn receive(&mut self, bytes: &mut [u8]) -> Result<()> {
        Ok(())
    }

    /*pub fn send(&self, dst_ipv4_addr: Ipv4Addr, payload: Vec<u8>) -> Future<'a, ()> {
        let rt = self.rt.clone();
        let arp = self.arp.clone();
        self.rt.start_task(move || {
            let mut fut = arp.query(dst_ipv4_addr);
            let dst_link_addr = {
                let mut dst_link_addr;
                loop {
                    match fut.poll(rt.clock()) {
                        Ok(a) => {
                            dst_link_addr = a;
                            break;
                        },
                        Err(Fail::TryAgain {}) => {
                            yield None;
                            continue;
                        },
                        x => return x,
                    }
                }

                dst_link_addr
            };

            /*let packet = UdpPacket::new(payload);
            {
                let ethernet2 = packet.ethernet2_mut();
            }*/

            Ok(())
        })
    }*/
}
