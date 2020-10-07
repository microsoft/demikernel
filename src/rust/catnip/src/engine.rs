// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    fail::Fail,
    protocols::{
        arp,
        ethernet::{
            self,
            MacAddress,
        },
        ip,
        ipv4,
        tcp::peer::SocketDescriptor,
        tcp::operations::{
            AcceptFuture,
            ConnectFuture,
            PopFuture,
            PushFuture,
        },
    },
    runtime::Runtime,
};
use bytes::Bytes;
use hashbrown::HashMap;
use std::{
    future::Future,
    net::Ipv4Addr,
    time::{
        Duration,
        Instant,
    },
};

pub struct Engine<RT: Runtime> {
    rt: RT,
    arp: arp::Peer<RT>,
    ipv4: ipv4::Peer<RT>,
}

impl<RT: Runtime> Engine<RT> {
    pub fn new(rt: RT) -> Result<Self, Fail> {
        let now = rt.now();
        let arp = arp::Peer::new(now, rt.clone())?;
        let ipv4 = ipv4::Peer::new(rt.clone(), arp.clone());
        Ok(Engine { rt, arp, ipv4 })
    }

    pub fn rt(&self) -> &RT {
        &self.rt
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), Fail> {
        let frame = ethernet::Frame::attach(&bytes)?;
        let header = frame.header();
        if self.rt.local_link_addr() != header.dest_addr() && !header.dest_addr().is_broadcast() {
            println!(
                "Misdelivered {:?} {:?} {}",
                self.rt.local_link_addr(),
                header.dest_addr(),
                header.dest_addr().is_broadcast()
            );
            return Err(Fail::Misdelivered {});
        }

        match header.ether_type()? {
            ethernet::EtherType::Arp => self.arp.receive(frame),
            ethernet::EtherType::Ipv4 => self.ipv4.receive(frame),
        }
    }

    pub fn tcp_socket(&mut self) -> SocketDescriptor {
        self.ipv4.tcp_socket()
    }

    pub fn arp_query(&self, ipv4_addr: Ipv4Addr) -> impl Future<Output = Result<MacAddress, Fail>> {
        self.arp.query(ipv4_addr)
    }

    pub fn udp_cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
        src_port: ip::Port,
        text: Vec<u8>,
    ) -> impl Future<Output = Result<(), Fail>> {
        self.ipv4
            .udp_cast(dest_ipv4_addr, dest_port, src_port, text)
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }

    pub fn import_arp_cache(&self, cache: HashMap<Ipv4Addr, MacAddress>) {
        self.arp.import_cache(cache)
    }

    pub fn ping(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        self.ipv4.ping(dest_ipv4_addr, timeout)
    }

    pub fn is_udp_port_open(&self, port: ip::Port) -> bool {
        self.ipv4.is_udp_port_open(port)
    }

    pub fn open_udp_port(&mut self, port: ip::Port) {
        self.ipv4.open_udp_port(port);
    }

    pub fn close_udp_port(&mut self, port: ip::Port) {
        self.ipv4.close_udp_port(port);
    }

    pub fn tcp_connect(
        &mut self,
        socket_fd: SocketDescriptor,
        remote_endpoint: ipv4::Endpoint,
    ) -> ConnectFuture<RT> {
        self.ipv4.tcp.connect(socket_fd, remote_endpoint)
    }

    pub fn tcp_push_async(&mut self, socket_fd: SocketDescriptor, buf: Bytes) -> PushFuture<RT> {
        self.ipv4.tcp.push_async(socket_fd, buf)
    }

    pub fn tcp_pop_async(&mut self, socket_fd: SocketDescriptor) -> PopFuture<RT> {
        self.ipv4.tcp.pop_async(socket_fd)
    }

    pub fn tcp_close(&mut self, socket_fd: SocketDescriptor) -> Result<(), Fail> {
        self.ipv4.tcp_close(socket_fd)
    }

    pub fn tcp_listen(&mut self, socket_fd: SocketDescriptor, backlog: usize) -> Result<(), Fail> {
        self.ipv4.tcp_listen(socket_fd, backlog)
    }

    pub fn tcp_bind(
        &mut self,
        socket_fd: SocketDescriptor,
        endpoint: ipv4::Endpoint,
    ) -> Result<(), Fail> {
        self.ipv4.tcp_bind(socket_fd, endpoint)
    }

    pub fn tcp_accept(
        &mut self,
        socket_fd: SocketDescriptor,
    ) -> Result<Option<SocketDescriptor>, Fail> {
        self.ipv4.tcp_accept(socket_fd)
    }

    pub fn tcp_write(&mut self, handle: SocketDescriptor, bytes: Bytes) -> Result<(), Fail> {
        self.ipv4.tcp_write(handle, bytes)
    }

    pub fn tcp_peek(&self, handle: SocketDescriptor) -> Result<Bytes, Fail> {
        self.ipv4.tcp_peek(handle)
    }

    pub fn tcp_read(&mut self, handle: SocketDescriptor) -> Result<Bytes, Fail> {
        self.ipv4.tcp_read(handle)
    }

    pub fn tcp_accept_async(&mut self, handle: SocketDescriptor) -> AcceptFuture<RT> {
        self.ipv4.tcp.accept_async(handle)
    }

    #[cfg(test)]
    pub fn tcp_mss(&self, handle: SocketDescriptor) -> Result<usize, Fail> {
        self.ipv4.tcp_mss(handle)
    }

    #[cfg(test)]
    pub fn tcp_rto(&self, handle: SocketDescriptor) -> Result<Duration, Fail> {
        self.ipv4.tcp_rto(handle)
    }
}
