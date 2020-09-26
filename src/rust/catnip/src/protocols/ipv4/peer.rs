// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use bytes::Bytes;
use super::datagram::{Ipv4Datagram, Ipv4Protocol};
use crate::protocols::tcp2::peer::SocketDescriptor;
use crate::{
    prelude::*,
    protocols::{arp, ethernet2, icmpv4, ip, ipv4, tcp2, udp},
};
use std::future::Future;
use std::{
    convert::TryFrom,
    net::Ipv4Addr,
    time::Duration, pin::Pin, task::{Poll, Context},
};
use crate::protocols::tcp2::runtime::Runtime as RuntimeTrait;

pub struct Ipv4Peer<RT: RuntimeTrait> {
    rt: RT,
    icmpv4: icmpv4::Peer<RT>,
    tcp: tcp2::Peer<RT>,
    udp: udp::Peer<RT>,
}

impl<RT: RuntimeTrait> Ipv4Peer<RT> {
    pub fn new(rt: RT, arp: arp::Peer<RT>) -> Ipv4Peer<RT> {
        let udp = udp::Peer::new(rt.clone(), arp.clone());
        let icmpv4 = icmpv4::Peer::new(rt.clone(), arp.clone());
        let tcp = tcp2::Peer::new(rt.clone(), arp);
        Ipv4Peer {
            rt,
            udp,
            icmpv4,
            tcp,
        }
    }

    pub fn receive(&mut self, frame: ethernet2::Frame<'_>) -> Result<()> {
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

    pub fn ping(&self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> impl Future<Output=Result<Duration>> {
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
    ) -> impl Future<Output=Result<()>> {
        self.udp.cast(dest_ipv4_addr, dest_port, src_port, text)
    }

    pub fn tcp_socket(&mut self) -> SocketDescriptor {
        self.tcp.socket()
    }

    pub fn tcp_connect(
        &mut self,
        remote_endpoint: ipv4::Endpoint,
    ) -> impl Future<Output=Result<SocketDescriptor>> {
        self.tcp.connect(remote_endpoint)
    }

    pub fn tcp_listen(&mut self, port: ip::Port) -> Result<u16> {
        let backlog = 256;
        let endpoint = ipv4::Endpoint::new(self.rt.local_ipv4_addr(), port);
        self.tcp.listen(endpoint, backlog)
    }

    pub fn tcp_listen2(&mut self, fd: SocketDescriptor, backlog: usize) -> Result<()> {
        self.tcp.listen2(fd, backlog)
    }

    pub fn tcp_bind(&mut self, fd: SocketDescriptor, endpoint: ipv4::Endpoint) -> Result<()> {
        self.tcp.bind(fd, endpoint)
    }

    pub fn tcp_accept(&mut self, fd: SocketDescriptor) -> Result<Option<SocketDescriptor>> {
        self.tcp.accept(fd)
    }

    pub fn tcp_write(
        &mut self,
        handle: SocketDescriptor,
        bytes: Bytes,
    ) -> Result<()> {
        self.tcp.send(handle, bytes)
    }

    pub fn tcp_peek(
        &self,
        handle: SocketDescriptor,
    ) -> Result<Bytes> {
        self.tcp.peek(handle)
    }

    pub fn tcp_read(
        &mut self,
        handle: SocketDescriptor,
    ) -> Result<Bytes> {
        match self.tcp.recv(handle)? {
            Some(r) => Ok(r),
            None => Err(Fail::ResourceExhausted { details: "No available data" }),
        }
    }

    pub fn tcp_close(&mut self, handle: SocketDescriptor) -> Result<()> {
        self.tcp.close(handle)
    }

    pub fn tcp_mss(&self, handle: SocketDescriptor) -> Result<usize> {
        self.tcp.remote_mss(handle)
    }

    pub fn tcp_rto(&self, handle: SocketDescriptor) -> Result<Duration> {
        self.tcp.current_rto(handle)
    }

    pub fn tcp_get_connection_id(
        &self,
        handle: SocketDescriptor,
    ) -> Result<(ipv4::Endpoint, ipv4::Endpoint)> {
        self.tcp.endpoints(handle)
    }
}

impl<RT: RuntimeTrait> Future for Ipv4Peer<RT> {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let self_ = self.get_mut();
        assert!(Future::poll(Pin::new(&mut self_.icmpv4), ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut self_.tcp), ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut self_.udp), ctx).is_pending());
        Poll::Pending
    }
}
