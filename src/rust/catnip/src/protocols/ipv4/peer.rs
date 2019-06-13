use super::{header::Ipv4Protocol, packet::Ipv4Packet};
use crate::{
    prelude::*,
    protocols::{arp, ethernet2, udp},
};
use crate::r#async::Future;
use std::net::Ipv4Addr;

pub struct Ipv4Peer<'a> {
    rt: Runtime<'a>,
    udp: udp::Peer<'a>,
}

impl<'a> Ipv4Peer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> Ipv4Peer<'a> {
        let udp = udp::Peer::new(rt.clone(), arp);
        Ipv4Peer { rt, udp }
    }

    pub fn receive(&mut self, frame: ethernet2::Frame) -> Result<()> {
        let options = self.rt.options();
        let packet = Ipv4Packet::from(frame);
        let header = packet.read_header()?;

        let dst_addr = header.dest_addr;
        if dst_addr != options.my_ipv4_addr && !dst_addr.is_broadcast() {
            return Err(Fail::Misdelivered {});
        }

        #[allow(unreachable_patterns)]
        match header.protocol {
            Ipv4Protocol::Udp => self.udp.receive(packet),
            _ => Err(Fail::Unsupported {}),
        }
    }

    pub fn udp_cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: u16,
        src_port: u16,
        payload: Vec<u8>,
    ) -> Future<'a, ()> {
        self.udp.cast(dest_ipv4_addr, dest_port, src_port, payload)
    }
}
