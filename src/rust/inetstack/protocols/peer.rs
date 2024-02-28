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
        SharedDemiRuntime,
        SharedBetweenCores,
    },
};
use ::std::{
    net::Ipv4Addr,
    time::Duration,
};

#[cfg(test)]
use crate::inetstack::protocols::tcp::socket::SharedTcpSocket;

pub struct Peer<N: NetworkRuntime> {
    local_ipv4_addr: Ipv4Addr,
    icmpv4: SharedIcmpv4Peer<N>,
    pub tcp: SharedTcpPeer<N>,
    pub udp: SharedUdpPeer<N>,
}

impl<N: NetworkRuntime> Peer<N> {
    pub fn new(
        runtime: SharedDemiRuntime,
        transport: N,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
        shared_between_cores: *mut SharedBetweenCores,
    ) -> Result<Self, Fail> {
        let udp_offload_checksum: bool = udp_config.get_tx_checksum_offload();
        let udp: SharedUdpPeer<N> = SharedUdpPeer::<N>::new(
            runtime.clone(),
            transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            udp_offload_checksum,
            arp.clone(),
        )?;
        let icmpv4: SharedIcmpv4Peer<N> = SharedIcmpv4Peer::<N>::new(
            runtime.clone(),
            transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp.clone(),
            rng_seed,
        )?;
        let tcp: SharedTcpPeer<N> = SharedTcpPeer::<N>::new(
            runtime.clone(),
            transport.clone(),
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng_seed,
            shared_between_cores
        )?;

        Ok(Peer {
            local_ipv4_addr,
            icmpv4,
            tcp,
            udp,
        })
    }

    pub fn receive(&mut self, buf: DemiBuffer) {
        let (header, payload) = match Ipv4Header::parse(buf) {
            Ok(result) => result,
            Err(e) => {
                let cause: String = format!("Invalid destination address: {:?}", e);
                warn!("dropping packet: {}", cause);
                return;
            },
        };
        debug!("Ipv4 received {:?}", header);
        if header.get_dest_addr() != self.local_ipv4_addr && !header.get_dest_addr().is_broadcast() {
            let cause: String = format!("Invalid destination address");
            warn!("dropping packet: {}", cause);
            return;
        }
        match header.get_protocol() {
            IpProtocol::ICMPv4 => self.icmpv4.receive(header, payload),
            IpProtocol::TCP => self.tcp.receive(header, payload),
            IpProtocol::UDP => self.udp.receive(header, payload),
        }
    }

    pub async fn ping(&mut self, dest_ipv4_addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.icmpv4.ping(dest_ipv4_addr, timeout).await
    }

    /// This function is only used for testing for now.
    /// TODO: Remove this function once our legacy tests have been disabled.
    pub fn get_local_addr(&self) -> Ipv4Addr {
        self.local_ipv4_addr
    }
}

#[cfg(test)]
impl<N: NetworkRuntime> Peer<N> {
    pub fn tcp_mss(&self, socket: &SharedTcpSocket<N>) -> Result<usize, Fail> {
        socket.remote_mss()
    }

    pub fn tcp_rto(&self, socket: &SharedTcpSocket<N>) -> Result<Duration, Fail> {
        socket.current_rto()
    }
}
