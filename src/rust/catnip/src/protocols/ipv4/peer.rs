// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
    rc::Rc,
    time::Duration, pin::Pin, task::{Poll, Context},
};

pub struct Ipv4Peer {
    rt: Runtime,
    icmpv4: icmpv4::Peer,
    tcp: tcp2::Peer<Runtime>,
    udp: udp::Peer,
}

impl<'a> Ipv4Peer {
    pub fn new(rt: Runtime, arp: arp::Peer) -> Ipv4Peer {
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
        let options = self.rt.options();
        let datagram = Ipv4Datagram::try_from(frame)?;
        let header = datagram.header();

        let dst_addr = header.dest_addr();
        if dst_addr != options.my_ipv4_addr && !dst_addr.is_broadcast() {
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

    pub fn tcp_connect(
        &mut self,
        remote_endpoint: ipv4::Endpoint,
    ) -> impl Future<Output=Result<SocketDescriptor>> {
        self.tcp.connect(remote_endpoint)
    }

    pub fn tcp_listen(&mut self, port: ip::Port) -> Result<u16> {
        let backlog = 256;
        let endpoint = ipv4::Endpoint::new(self.rt.options().my_ipv4_addr, port);
        self.tcp.listen(endpoint, backlog)
    }

    pub fn tcp_write(
        &mut self,
        handle: SocketDescriptor,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.tcp.send(handle, bytes)
    }

    pub fn tcp_peek(
        &self,
        handle: SocketDescriptor,
    ) -> Result<Rc<Vec<u8>>> {
        Ok(Rc::new(self.tcp.peek(handle)?))
    }

    pub fn tcp_read(
        &mut self,
        handle: SocketDescriptor,
    ) -> Result<Rc<Vec<u8>>> {
        match self.tcp.recv(handle)? {
            Some(r) => Ok(Rc::new(r)),
            None => Err(Fail::ResourceExhausted { details: "No available data" }),
        }
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

impl Future for Ipv4Peer {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let self_ = self.get_mut();
        assert!(Future::poll(Pin::new(&mut self_.icmpv4), ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut self_.tcp), ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut self_.udp), ctx).is_pending());
        Poll::Pending
    }
}
