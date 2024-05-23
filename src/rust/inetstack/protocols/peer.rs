// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    demikernel::config::Config,
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
        network::NetworkRuntime,
        SharedDemiRuntime,
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
        config: &Config,
        runtime: SharedDemiRuntime,
        transport: N,
        arp: SharedArpPeer<N>,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let udp: SharedUdpPeer<N> = SharedUdpPeer::<N>::new(config, runtime.clone(), transport.clone(), arp.clone())?;
        let icmpv4: SharedIcmpv4Peer<N> =
            SharedIcmpv4Peer::<N>::new(config, runtime.clone(), transport.clone(), arp.clone(), rng_seed)?;
        let tcp: SharedTcpPeer<N> = SharedTcpPeer::<N>::new(config, runtime.clone(), transport.clone(), arp, rng_seed)?;

        Ok(Peer {
            local_ipv4_addr: config.local_ipv4_addr()?,
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
