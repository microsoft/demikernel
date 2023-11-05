// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::{
        arp::SharedArpPeer,
        icmpv4::SharedIcmpv4Peer,
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::SharedTcpPeer,
        udp::SharedUdpPeer,
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
        SharedBox,
        SharedDemiRuntime,
    },
};
use ::libc::ENOTCONN;
use ::std::{
    net::Ipv4Addr,
    time::Duration,
};

#[cfg(test)]
use crate::runtime::QDesc;

pub struct Peer<const N: usize> {
    local_ipv4_addr: Ipv4Addr,
    icmpv4: SharedIcmpv4Peer<N>,
    pub tcp: SharedTcpPeer<N>,
    pub udp: SharedUdpPeer<N>,
}

impl<const N: usize> Peer<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let udp_offload_checksum: bool = udp_config.get_tx_checksum_offload();
        let udp: SharedUdpPeer<N> = SharedUdpPeer::new(
            runtime.clone(),
            transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            udp_offload_checksum,
            arp.clone(),
        )?;
        let icmpv4: SharedIcmpv4Peer<N> = SharedIcmpv4Peer::new(
            runtime.clone(),
            transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp.clone(),
            rng_seed,
        )?;
        let tcp: SharedTcpPeer<N> = SharedTcpPeer::new(
            runtime.clone(),
            transport.clone(),
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
            IpProtocol::UDP => self.udp.receive(&header, payload),
        }
    }

    pub async fn ping(&mut self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.icmpv4.ping(dest_ipv4_addr, timeout).await
    }
}

#[cfg(test)]
impl<const N: usize> Peer<N> {
    pub fn tcp_mss(&self, fd: QDesc) -> Result<usize, Fail> {
        self.tcp.remote_mss(fd)
    }

    pub fn tcp_rto(&self, fd: QDesc) -> Result<Duration, Fail> {
        self.tcp.current_rto(fd)
    }
}
