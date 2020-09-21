// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    prelude::*,
    protocols::{
        arp,
        ethernet2::{self, MacAddress},
        ip, ipv4,
    },
};
use bytes::Bytes;
use crate::event::Event;
use crate::protocols::tcp2::peer::SocketDescriptor;
use futures::task::{Context, noop_waker_ref};
use fxhash::FxHashMap;
use std::future::Future;
use std::{
    net::Ipv4Addr,
    rc::Rc,
    time::{Duration, Instant},
};
use std::pin::Pin;

pub struct Engine {
    rt: Runtime,
    arp: arp::Peer,
    ipv4: ipv4::Peer,

    // TODO: Hax to support upper layer not calling `accept`.
    listening: Vec<SocketDescriptor>,
}

impl Engine {
    pub fn from_options(now: Instant, options: Options) -> Result<Engine> {
        let rt = Runtime::from_options(now, options);
        let arp = arp::Peer::new(now, rt.clone())?;
        let ipv4 = ipv4::Peer::new(rt.clone(), arp.clone());
        Ok(Engine { rt, arp, ipv4, listening: vec![] })
    }

    pub fn options(&self) -> Options {
        self.rt.options()
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        trace!("Engine::receive({:?})", bytes);
        let frame = ethernet2::Frame::attach(&bytes)?;
        let header = frame.header();
        if self.rt.options().my_link_addr != header.dest_addr()
            && !header.dest_addr().is_broadcast()
        {
            return Err(Fail::Misdelivered {});
        }

        match header.ether_type()? {
            ethernet2::EtherType::Arp => self.arp.receive(frame),
            ethernet2::EtherType::Ipv4 => self.ipv4.receive(frame),
        }
    }

    pub fn arp_query(
        &self,
        ipv4_addr: Ipv4Addr,
    ) -> impl Future<Output=Result<MacAddress>> {
        self.arp.query(ipv4_addr)
    }

    pub fn udp_cast(
        &self,
        dest_ipv4_addr: Ipv4Addr,
        dest_port: ip::Port,
        src_port: ip::Port,
        text: Vec<u8>,
    ) -> impl Future<Output=Result<()>> {
        self.ipv4
            .udp_cast(dest_ipv4_addr, dest_port, src_port, text)
    }

    pub fn export_arp_cache(&self) -> FxHashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }

    pub fn import_arp_cache(&self, cache: FxHashMap<Ipv4Addr, MacAddress>) {
        self.arp.import_cache(cache)
    }

    pub fn ping(&self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> impl Future<Output=Result<Duration>> {
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

    pub fn tcp_connect(&mut self, remote_endpoint: ipv4::Endpoint) -> impl Future<Output=Result<SocketDescriptor>> {
        self.ipv4.tcp_connect(remote_endpoint)
    }

    pub fn tcp_listen(&mut self, port: ip::Port) -> Result<()> {
        self.listening.push(self.ipv4.tcp_listen(port)?);
        Ok(())
    }

    pub fn tcp_write(
        &mut self,
        handle: SocketDescriptor,
        bytes: Bytes,
    ) -> Result<()> {
        self.ipv4.tcp_write(handle, bytes)
    }

    pub fn tcp_peek(
        &self,
        handle: SocketDescriptor,
    ) -> Result<Rc<Vec<u8>>> {
        self.ipv4.tcp_peek(handle)
    }

    pub fn tcp_read(
        &mut self,
        handle: SocketDescriptor,
    ) -> Result<Rc<Vec<u8>>> {
        self.ipv4.tcp_read(handle)
    }

    pub fn tcp_mss(&self, handle: SocketDescriptor) -> Result<usize> {
        self.ipv4.tcp_mss(handle)
    }

    pub fn tcp_rto(&self, handle: SocketDescriptor) -> Result<Duration> {
        self.ipv4.tcp_rto(handle)
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.rt.advance_clock(now);

        let mut ctx = Context::from_waker(noop_waker_ref());
        assert!(Future::poll(Pin::new(&mut self.rt), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut self.arp), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut self.ipv4), &mut ctx).is_pending());

        for &socket_fd in &self.listening {
            loop {
                match self.ipv4.tcp_accept(socket_fd) {
                    Ok(Some(fd)) => {
                        self.rt.emit_event(Event::IncomingTcpConnection(fd));
                    },
                    Ok(None) => break,
                    Err(e) => {
                        warn!("Accept failed on {:?}: {:?}", socket_fd, e);
                    }
                }
            }
        }
    }

    pub fn next_event(&self) -> Option<Rc<Event>> {
        self.rt.next_event()
    }

    pub fn pop_event(&self) -> Option<Rc<Event>> {
        self.rt.pop_event()
    }

    pub fn tcp_get_connection_id(
        &self,
        handle: SocketDescriptor,
    ) -> Result<(ipv4::Endpoint, ipv4::Endpoint)> {
        self.ipv4.tcp_get_connection_id(handle)
    }
}
