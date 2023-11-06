// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::{
        protocols::{
            arp::SharedArpPeer,
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            udp::SharedUdpPeer,
            Peer,
        },
        ArpConfig,
        TcpConfig,
        UdpConfig,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        Operation,
        QDesc,
        SharedBox,
        SharedObject,
    },
};
use ::libc::EBADMSG;
use ::std::{
    collections::HashMap,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    time::{
        Duration,
        Instant,
    },
};

use super::SharedTestRuntime;

pub struct Engine<const N: usize> {
    test_rig: SharedTestRuntime,
    arp: SharedArpPeer<N>,
    ipv4: Peer<N>,
}

#[derive(Clone)]
pub struct SharedEngine<const N: usize>(SharedObject<Engine<N>>);

impl<const N: usize> SharedEngine<N> {
    pub fn new(test_rig: SharedTestRuntime) -> Result<Self, Fail> {
        let link_addr: MacAddress = test_rig.get_link_addr();
        let ipv4_addr: Ipv4Addr = test_rig.get_ip_addr();
        let arp_config: ArpConfig = test_rig.get_arp_config();
        let udp_config: UdpConfig = test_rig.get_udp_config();
        let tcp_config: TcpConfig = test_rig.get_tcp_config();

        let boxed_test_rig: SharedBox<dyn NetworkRuntime<N>> = SharedBox::new(Box::new(test_rig.clone()));
        let arp = SharedArpPeer::new(
            test_rig.get_runtime(),
            boxed_test_rig.clone(),
            link_addr,
            ipv4_addr,
            arp_config,
        )?;
        let rng_seed: [u8; 32] = [0; 32];
        let ipv4 = Peer::new(
            test_rig.get_runtime(),
            boxed_test_rig.clone(),
            link_addr,
            ipv4_addr,
            udp_config,
            tcp_config,
            arp.clone(),
            rng_seed,
        )?;
        Ok(Self(SharedObject::<Engine<N>>::new(Engine { test_rig, arp, ipv4 })))
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.test_rig.advance_clock(now)
    }

    pub fn receive(&mut self, bytes: DemiBuffer) -> Result<(), Fail> {
        let (header, payload) = Ethernet2Header::parse(bytes)?;
        debug!("Engine received {:?}", header);
        if self.test_rig.get_link_addr() != header.dst_addr() && !header.dst_addr().is_broadcast() {
            return Err(Fail::new(EBADMSG, "physical destination address mismatch"));
        }
        match header.ether_type() {
            EtherType2::Arp => self.arp.receive(payload),
            EtherType2::Ipv4 => self.ipv4.receive(payload),
            EtherType2::Ipv6 => Ok(()), // Ignore for now.
        }
    }

    pub async fn ipv4_ping(&mut self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.ipv4.ping(dest_ipv4_addr, timeout).await
    }

    pub fn udp_pushto(&self, qd: QDesc, buf: DemiBuffer, to: SocketAddrV4) -> Result<(), Fail> {
        let mut udp: SharedUdpPeer<N> = self.ipv4.udp.clone();
        udp.pushto(qd, buf, to)
    }

    pub fn udp_pop(&self, qd: QDesc) -> Pin<Box<Operation>> {
        let mut udp: SharedUdpPeer<N> = self.ipv4.udp.clone();
        Box::pin(async move { udp.pop_coroutine(qd, None).await })
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
        self.ipv4.tcp.socket()
    }

    pub fn tcp_connect(&mut self, socket_fd: QDesc, remote_endpoint: SocketAddrV4) -> Pin<Box<Operation>> {
        self.ipv4.tcp.connect(socket_fd, remote_endpoint)
    }

    pub fn tcp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.ipv4.tcp.bind(socket_fd, endpoint)
    }

    pub fn tcp_accept(&self, fd: QDesc) -> Pin<Box<Operation>> {
        self.ipv4.tcp.accept(fd)
    }

    pub fn tcp_push(&mut self, socket_fd: QDesc, buf: DemiBuffer) -> Pin<Box<Operation>> {
        self.ipv4.tcp.push(socket_fd, buf)
    }

    pub fn tcp_pop(&mut self, socket_fd: QDesc) -> Pin<Box<Operation>> {
        self.ipv4.tcp.pop(socket_fd, None)
    }

    pub fn tcp_close(&mut self, socket_fd: QDesc) -> Result<(), Fail> {
        self.ipv4.tcp.close(socket_fd)
    }

    pub fn tcp_listen(&mut self, socket_fd: QDesc, backlog: usize) -> Result<(), Fail> {
        self.ipv4.tcp.listen(socket_fd, backlog)
    }

    pub async fn arp_query(&mut self, ipv4_addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.arp.query(ipv4_addr).await
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

    pub fn get_test_rig(&self) -> SharedTestRuntime {
        self.test_rig.clone()
    }
}

impl<const N: usize> Deref for SharedEngine<N> {
    type Target = Engine<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedEngine<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
