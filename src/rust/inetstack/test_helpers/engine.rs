// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        queue::InetQueue,
        tcp::operations::{
            AcceptFuture,
            ConnectFuture,
            PopFuture,
            PushFuture,
        },
        udp::UdpPopFuture,
        Peer,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::types::MacAddress,
        queue::IoQueueTable,
        timer::TimerRc,
        QDesc,
    },
    scheduler::scheduler::Scheduler,
};
use ::libc::EBADMSG;
use ::std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    time::Duration,
};

use super::TestRuntime;

pub struct Engine {
    pub rt: Rc<TestRuntime>,
    pub clock: TimerRc,
    pub arp: ArpPeer,
    pub ipv4: Peer,
    pub qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
}

impl Engine {
    pub fn new(rt: TestRuntime, scheduler: Scheduler, clock: TimerRc) -> Result<Self, Fail> {
        let rt = Rc::new(rt);
        let link_addr = rt.link_addr;
        let ipv4_addr = rt.ipv4_addr;
        let arp_options = rt.arp_options.clone();
        let udp_config = rt.udp_config.clone();
        let tcp_config = rt.tcp_config.clone();
        let qtable = Rc::new(RefCell::new(IoQueueTable::<InetQueue>::new()));
        let arp = ArpPeer::new(
            rt.clone(),
            scheduler.clone(),
            clock.clone(),
            link_addr,
            ipv4_addr,
            arp_options,
        )?;
        let rng_seed: [u8; 32] = [0; 32];
        let ipv4 = Peer::new(
            rt.clone(),
            scheduler.clone(),
            qtable.clone(),
            clock.clone(),
            link_addr,
            ipv4_addr,
            udp_config,
            tcp_config,
            arp.clone(),
            rng_seed,
        )?;
        Ok(Engine {
            rt,
            clock,
            arp,
            ipv4,
            qtable,
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

    pub fn udp_pushto(&self, fd: QDesc, buf: DemiBuffer, to: SocketAddrV4) -> Result<(), Fail> {
        self.ipv4.udp.do_pushto(fd, buf, to)
    }

    pub fn udp_pop(&mut self, fd: QDesc) -> UdpPopFuture {
        self.ipv4.udp.do_pop(fd, None)
    }

    pub fn udp_socket(&mut self) -> Result<QDesc, Fail> {
        self.ipv4.udp.do_socket()
    }

    pub fn udp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.ipv4.udp.do_bind(socket_fd, endpoint)
    }

    pub fn udp_close(&mut self, socket_fd: QDesc) -> Result<(), Fail> {
        self.ipv4.udp.do_close(socket_fd)
    }

    pub fn tcp_socket(&mut self) -> Result<QDesc, Fail> {
        self.ipv4.tcp.do_socket()
    }

    pub fn tcp_connect(&mut self, socket_fd: QDesc, remote_endpoint: SocketAddrV4) -> ConnectFuture {
        self.ipv4.tcp.connect(socket_fd, remote_endpoint).unwrap()
    }

    pub fn tcp_bind(&mut self, socket_fd: QDesc, endpoint: SocketAddrV4) -> Result<(), Fail> {
        self.ipv4.tcp.bind(socket_fd, endpoint)
    }

    pub fn tcp_accept(&mut self, fd: QDesc) -> AcceptFuture {
        let (_, future) = self.ipv4.tcp.do_accept(fd);
        future
    }

    pub fn tcp_push(&mut self, socket_fd: QDesc, buf: DemiBuffer) -> PushFuture {
        self.ipv4.tcp.push(socket_fd, buf)
    }

    pub fn tcp_pop(&mut self, socket_fd: QDesc) -> PopFuture {
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
