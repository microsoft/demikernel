// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::{Ipv4Datagram, Ipv4Protocol};
use crate::{
    prelude::*,
    protocols::{arp, ethernet2, icmpv4, ip, ipv4, tcp, udp},
};
use std::{
    convert::TryFrom,
    net::Ipv4Addr,
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Ipv4Peer<'a> {
    rt: Runtime<'a>,
    icmpv4: icmpv4::Peer<'a>,
    tcp: tcp::Peer<'a>,
    udp: udp::Peer<'a>,
}

impl<'a> Ipv4Peer<'a> {
    pub fn new(rt: Runtime<'a>, arp: arp::Peer<'a>) -> Ipv4Peer<'a> {
        let udp = udp::Peer::new(rt.clone(), arp.clone());
        let icmpv4 = icmpv4::Peer::new(rt.clone(), arp.clone());
        let tcp = tcp::Peer::new(rt.clone(), arp);
        Ipv4Peer {
            rt,
            udp,
            icmpv4,
            tcp,
        }
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

        match header.protocol()? {
            Ipv4Protocol::Tcp => self.tcp.receive(datagram),
            Ipv4Protocol::Udp => self.udp.receive(datagram),
            Ipv4Protocol::Icmpv4 => self.icmpv4.receive(datagram),
        }
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> Future<'a, Duration> {
        self.icmpv4.ping(dest_ipv4_addr, timeout)
    }

    pub fn is_udp_port_open(&self, port: ip::Port) -> bool {
        self.udp.is_port_open(port)
    }

    pub fn open_udp_port(&mut self, port: ip::Port) {
        self.udp.open_port(port);
    }

    pub fn close_udp_port(&mut self, port: ip::Port) {
        self.udp.close_port(port);
    }

    pub fn udp_cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
        src_port: ip::Port,
        text: Vec<u8>,
    ) -> Future<'a, ()> {
        self.udp.cast(dest_ipv4_addr, dest_port, src_port, text)
    }

    pub fn tcp_connect(
        &mut self,
        remote_endpoint: ipv4::Endpoint,
    ) -> Future<'a, tcp::ConnectionHandle> {
        tcp::Peer::connect(&self.tcp, remote_endpoint)
    }

    pub fn tcp_listen(&mut self, port: ip::Port) -> Result<()> {
        self.tcp.listen(port)
    }

    pub fn tcp_write(
        &mut self,
        handle: tcp::ConnectionHandle,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.tcp.write(handle, bytes)
    }

    pub fn tcp_peek(
        &self,
        handle: tcp::ConnectionHandle,
    ) -> Result<Rc<Vec<u8>>> {
        self.tcp.peek(handle)
    }

    pub fn tcp_read(
        &mut self,
        handle: tcp::ConnectionHandle,
    ) -> Result<Rc<Vec<u8>>> {
        self.tcp.read(handle)
    }

    pub fn tcp_mss(&self, handle: tcp::ConnectionHandle) -> Result<usize> {
        self.tcp.get_mss(handle)
    }

    pub fn tcp_rto(&self, handle: tcp::ConnectionHandle) -> Result<Duration> {
        self.tcp.get_rto(handle)
    }

    pub fn advance_clock(&self, now: Instant) {
        self.icmpv4.advance_clock(now);
        self.udp.advance_clock(now);
        self.tcp.advance_clock(now);
    }

    pub fn tcp_get_connection_id(
        &self,
        handle: tcp::ConnectionHandle,
    ) -> Result<Rc<tcp::ConnectionId>> {
        self.tcp.get_connection_id(handle)
    }
}
