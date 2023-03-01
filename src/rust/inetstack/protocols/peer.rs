// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        icmpv4::Icmpv4Peer,
        ip::IpProtocol,
        ipv4::Ipv4Header,
        queue::InetQueue,
        tcp::TcpPeer,
        udp::UdpPeer,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            config::{
                TcpConfig,
                UdpConfig,
            },
            types::MacAddress,
            NetworkRuntime,
        },
        queue::IoQueueTable,
        timer::TimerRc,
    },
    scheduler::scheduler::Scheduler,
};
use ::libc::ENOTCONN;
use ::std::{
    cell::RefCell,
    future::Future,
    net::Ipv4Addr,
    rc::Rc,
    time::Duration,
};

#[cfg(test)]
use crate::runtime::QDesc;

pub struct Peer {
    local_ipv4_addr: Ipv4Addr,
    icmpv4: Icmpv4Peer,
    pub tcp: TcpPeer,
    pub udp: UdpPeer,
}

impl Peer {
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        qtable: Rc<RefCell<IoQueueTable<InetQueue>>>,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        rng_seed: [u8; 32],
    ) -> Result<Peer, Fail> {
        let udp_offload_checksum: bool = udp_config.get_tx_checksum_offload();
        let udp: UdpPeer = UdpPeer::new(
            rt.clone(),
            scheduler.clone(),
            qtable.clone(),
            rng_seed,
            local_link_addr,
            local_ipv4_addr,
            udp_offload_checksum,
            arp.clone(),
        )?;
        let icmpv4: Icmpv4Peer = Icmpv4Peer::new(
            rt.clone(),
            scheduler.clone(),
            clock.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp.clone(),
            rng_seed,
        )?;
        let tcp: TcpPeer = TcpPeer::new(
            rt.clone(),
            scheduler.clone(),
            qtable.clone(),
            clock.clone(),
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng_seed,
        )?;

        Ok(Peer {
            local_ipv4_addr,
            icmpv4,
            tcp,
            udp,
        })
    }

    pub fn receive(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        let (header, payload) = Ipv4Header::parse(buf)?;
        debug!("Ipv4 received {:?}", header);
        if header.get_dest_addr() != self.local_ipv4_addr && !header.get_dest_addr().is_broadcast() {
            return Err(Fail::new(ENOTCONN, "invalid destination address"));
        }
        match header.get_protocol() {
            IpProtocol::ICMPv4 => self.icmpv4.receive(&header, payload),
            IpProtocol::TCP => self.tcp.receive(&header, payload),
            IpProtocol::UDP => self.udp.do_receive(&header, payload),
        }
    }

    pub fn ping(
        &mut self,
        dest_ipv4_addr: Ipv4Addr,
        timeout: Option<Duration>,
    ) -> impl Future<Output = Result<Duration, Fail>> {
        self.icmpv4.ping(dest_ipv4_addr, timeout)
    }
}

#[cfg(test)]
impl Peer {
    pub fn tcp_mss(&self, fd: QDesc) -> Result<usize, Fail> {
        self.tcp.remote_mss(fd)
    }

    pub fn tcp_rto(&self, fd: QDesc) -> Result<Duration, Fail> {
        self.tcp.current_rto(fd)
    }
}
