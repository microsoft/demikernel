// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        tcp::operations::{
            AcceptFuture,
            ConnectFuture,
            PopFuture,
            PushFuture,
        },
        udp::SharedUdpPeer,
        Peer,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        timer::TimerRc,
        QDesc,
        SharedDemiRuntime,
    },
};
use ::libc::EBADMSG;
use ::std::{
    collections::HashMap,
    future::Future,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    time::Duration,
};

use super::TestRuntime;

pub struct Engine<const N: usize> {
    pub transport: SharedTestRuntime,
    pub clock: TimerRc,
    pub arp: ArpPeer<N>,
    pub ipv4: Peer<N>,
    pub runtime: SharedDemiRuntime,
}

impl<const N: usize> Engine<N> {
    pub fn new(transport: SharedTestRuntime, runtime: SharedDemiRuntime, clock: TimerRc) -> Result<Self, Fail> {
        let link_addr = transport.link_addr;
        let ipv4_addr = transport.ipv4_addr;
        let arp_options = transport.arp_options.clone();
        let udp_config = transport.udp_config.clone();
        let tcp_config = transport.tcp_config.clone();
        let arp = SharedArpPeer::new(
            runtime.clone(),
            transport.clone(),
            clock.clone(),
            link_addr,
            ipv4_addr,
            arp_options,
        )?;
        let rng_seed: [u8; 32] = [0; 32];
        let ipv4 = Peer::new(
            runtime.clone(),
            transport.clone(),
            clock.clone(),
            link_addr,
            ipv4_addr,
            udp_config,
            tcp_config,
            arp.clone(),
            rng_seed,
        )?;
        Ok(Engine {
            transport,
            clock,
            arp,
            ipv4,
            runtime,
        })
    }

    pub fn receive(&mut self, bytes: DemiBuffer) -> Result<(), Fail> {
        let (header, payload) = Ethernet2Header::parse(bytes)?;
        debug!("Engine received {:?}", header);
        if self.rt.link_addr != header.dst_addr() && !header.dst_addr().is_broadcast() {
            return Err(Fail::new(EBADMSG, "physical destination address mismatch"));
        }
        match header.ether_type() {
            EtherType2::Arp => self.arp.receive(payload),
            EtherType2::Ipv4 => self.ipv4.receive(payload),
            EtherType2::Ipv6 => Ok(()), // Ignore for now.
        }
    }

    pub fn ipv4_ping(
        &mut self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        self.ipv4.ping(dest_ipv4_addr, timeout)
    }

    pub fn udp_pushto(&self, qd: QDesc, buf: DemiBuffer, to: SocketAddrV4) -> Result<(), Fail> {
        let mut udp: SharedUdpPeer<N> = self.ipv4.udp.clone();
        udp.pushto(qd, buf, to)
    }

    pub fn udp_pop(&self, qd: QDesc) -> Pin<Box<Operation>> {
        let mut udp: SharedUdpPeer<N> = self.ipv4.udp.clone();
        Box::pin(async move { udp.pop(qd, None).await })
    }

    pub fn udp_socket(&mut self) -> Result<QDesc, Fail> {
        self.ipv4.udp.socket()
    }

    pub fn udp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.ipv4.udp.bind(socket_fd, endpoint)
    }

    pub fn udp_close(&mut self, socket_fd: QDesc) -> Result<(), Fail> {
        self.ipv4.udp.close(socket_fd)
    }

    pub fn tcp_socket(&mut self) -> Result<QDesc, Fail> {
        self.ipv4.tcp.do_socket()
    }

    pub fn tcp_connect(&mut self, socket_fd: QDesc, remote_endpoint: SocketAddrV4) -> ConnectFuture<N> {
        self.ipv4.tcp.connect(socket_fd, remote_endpoint).unwrap()
    }

    pub fn tcp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.ipv4.tcp.bind(socket_fd, endpoint)
    }

    pub fn tcp_accept(&mut self, fd: QDesc) -> AcceptFuture<N> {
        let (_, future) = self.ipv4.tcp.do_accept(fd);
        future
    }

    pub fn tcp_push(&mut self, socket_fd: QDesc, buf: DemiBuffer) -> PushFuture {
        self.ipv4.tcp.push(socket_fd, buf)
    }

    pub fn tcp_pop(&mut self, socket_fd: QDesc) -> PopFuture<N> {
        self.ipv4.tcp.pop(socket_fd, None)
    }

    pub fn tcp_close(&mut self, socket_fd: QDesc) -> Result<(), Fail> {
        self.ipv4.tcp.do_close(socket_fd)
    }

    pub fn tcp_listen(&mut self, socket_fd: QDesc, backlog: usize) -> Result<(), Fail> {
        self.ipv4.tcp.listen(socket_fd, backlog)
    }

    pub fn arp_query(&self, ipv4_addr: Ipv4Addr) -> impl Future<Output = Result<MacAddress, Fail>> {
        self.arp.query(ipv4_addr)
    }

    pub fn tcp_mss(&self, handle: QDesc) -> Result<usize, Fail> {
        self.ipv4.tcp_mss(handle)
    }

    pub fn tcp_rto(&self, handle: QDesc) -> Result<Duration, Fail> {
        self.ipv4.tcp_rto(handle)
    }

    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress> {
        self.arp.export_cache()
    }
}
