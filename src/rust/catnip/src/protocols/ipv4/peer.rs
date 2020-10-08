// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::{
    Ipv4Datagram,
    Ipv4Protocol,
};
use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet,
        icmpv4,
        ip,
        tcp,
        udp,
    },
    runtime::Runtime,
};
use std::{
    convert::TryFrom,
    future::Future,
    net::Ipv4Addr,
    time::Duration,
};
use crate::file_table::FileTable;
#[cfg(test)]
use crate::file_table::FileDescriptor;

pub struct Ipv4Peer<RT: Runtime> {
    rt: RT,
    icmpv4: icmpv4::Peer<RT>,
    pub tcp: tcp::Peer<RT>,
    udp: udp::Peer<RT>,
}

impl<RT: Runtime> Ipv4Peer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>, file_table: FileTable) -> Ipv4Peer<RT> {
        let udp = udp::Peer::new(rt.clone(), arp.clone(), file_table.clone());
        let icmpv4 = icmpv4::Peer::new(rt.clone(), arp.clone());
        let tcp = tcp::Peer::new(rt.clone(), arp, file_table);
        Ipv4Peer {
            rt,
            udp,
            icmpv4,
            tcp,
        }
    }

    pub fn receive(&mut self, frame: ethernet::Frame<'_>) -> Result<(), Fail> {
        trace!("Ipv4Peer::receive(...)");
        let datagram = Ipv4Datagram::try_from(frame)?;
        let header = datagram.header();

        let dst_addr = header.dest_addr();
        if dst_addr != self.rt.local_ipv4_addr() && !dst_addr.is_broadcast() {
            return Err(Fail::Misdelivered {});
        }

        match header.protocol()? {
            Ipv4Protocol::Tcp => {
                self.tcp.receive_datagram(datagram);
                Ok(())
            },
            Ipv4Protocol::Udp => self.udp.receive(datagram),
            Ipv4Protocol::Icmpv4 => self.icmpv4.receive(datagram),
        }
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
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
    ) -> impl Future<Output = Result<(), Fail>> {
        self.udp.cast(dest_ipv4_addr, dest_port, src_port, text)
    }
}

#[cfg(test)]
impl<RT: Runtime> Ipv4Peer<RT> {
    pub fn tcp_mss(&self, fd: FileDescriptor) -> Result<usize, Fail> {
        self.tcp.remote_mss(fd)
    }

    pub fn tcp_rto(&self, fd: FileDescriptor) -> Result<Duration, Fail> {
        self.tcp.current_rto(fd)
    }
}
