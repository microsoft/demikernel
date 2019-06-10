use super::{header::Ipv4Protocol, packet::Ipv4PacketMut};
use crate::{
    prelude::*,
    protocols::{arp, ethernet2, udp},
};
use std::convert::TryFrom;

pub struct Ipv4Peer<'a> {
    rt: Runtime<'a>,
    udp: udp::Peer<'a>,
}

impl<'a> Ipv4Peer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> Ipv4Peer<'a> {
        let udp = udp::Peer::new(rt.clone(), arp);
        Ipv4Peer { rt, udp }
    }

    pub fn receive(&mut self, bytes: &mut [u8]) -> Result<()> {
        let mut packet = Ipv4PacketMut::from_bytes(bytes)?;
        let options = self.rt.options();

        let dst_addr = packet.header_mut().get_dst_addr();
        if dst_addr != options.my_ipv4_addr && !dst_addr.is_broadcast() {
            return Err(Fail::Misdelivered {});
        }

        #[allow(unreachable_patterns)]
        match Ipv4Protocol::try_from(packet.header_mut().get_proto())? {
            Ipv4Protocol::Udp => self.udp.receive(bytes),
            _ => Err(Fail::Unsupported {}),
        }
    }
}
