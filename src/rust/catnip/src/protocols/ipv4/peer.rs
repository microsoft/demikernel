use super::datagram::{Ipv4Datagram, Ipv4Protocol};
use crate::{
    prelude::*,
    protocols::{arp, ethernet2, icmpv4, udp},
    r#async::Future,
};
use std::{
    convert::TryFrom,
    net::Ipv4Addr,
    time::{Duration, Instant},
};

pub struct Ipv4Peer<'a> {
    rt: Runtime<'a>,
    udp: udp::Peer<'a>,
    icmpv4: icmpv4::Peer<'a>,
}

impl<'a> Ipv4Peer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> Ipv4Peer<'a> {
        let udp = udp::Peer::new(rt.clone(), arp.clone());
        let icmpv4 = icmpv4::Peer::new(rt.clone(), arp);
        Ipv4Peer { rt, udp, icmpv4 }
    }

    pub fn receive(&mut self, frame: ethernet2::Frame<'_>) -> Result<()> {
        trace!("Ipv4Peer::receive(...)");
        let options = self.rt.options();
        let datagram = Ipv4Datagram::try_from(frame)?;
        let header = datagram.header();

        let dst_addr = header.dest_addr();
        if dst_addr != options.my_ipv4_addr && !dst_addr.is_broadcast() {
            return Err(Fail::Misdelivered {});
        }

        #[allow(unreachable_patterns)]
        match header.protocol()? {
            Ipv4Protocol::Icmpv4 => self.icmpv4.receive(datagram),
            Ipv4Protocol::Udp => self.udp.receive(datagram),
            _ => Err(Fail::Unimplemented {}),
        }
    }

    pub fn poll(&mut self, now: Instant) -> Result<()> {
        Ok(self.icmpv4.poll(now)?)
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> Future<'a, Duration> {
        self.icmpv4.ping(dest_ipv4_addr, timeout)
    }

    pub fn is_udp_port_open(&self, port_num: u16) -> bool {
        self.udp.is_port_open(port_num)
    }

    pub fn open_udp_port(&mut self, port_num: u16) {
        self.udp.open_port(port_num);
    }

    pub fn close_udp_port(&mut self, port_num: u16) {
        self.udp.close_port(port_num);
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
