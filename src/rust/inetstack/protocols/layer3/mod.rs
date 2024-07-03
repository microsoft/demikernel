// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

pub mod arp;
pub mod icmpv4;
pub mod ip;
pub mod ipv4;

pub use self::{
    arp::SharedArpPeer,
    icmpv4::SharedIcmpv4Peer,
    ip::IpProtocol,
    ipv4::Ipv4Header,
};

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(test)]
use crate::MacAddress;
use crate::{
    demikernel::config::Config,
    inetstack::protocols::layer2::{
        EtherType2,
        Ethernet2Header,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::NetworkRuntime,
        SharedDemiRuntime,
    },
};
use ::std::net::Ipv4Addr;
#[cfg(test)]
use ::std::{
    collections::HashMap,
    hash::RandomState,
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer3Endpoint<N: NetworkRuntime> {
    // TODO: Remove this once we have proper separation between layers.
    arp: SharedArpPeer<N>,
    icmpv4: SharedIcmpv4Peer<N>,
    local_ipv4_addr: Ipv4Addr,
}

//======================================================================================================================
// Associated Functions

//======================================================================================================================

impl<N: NetworkRuntime> Layer3Endpoint<N> {
    pub fn new(
        config: &Config,
        runtime: SharedDemiRuntime,
        arp: SharedArpPeer<N>,
        layer1_endpoint: N,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        Ok(Self {
            arp: arp.clone(),
            icmpv4: SharedIcmpv4Peer::<N>::new(&config, runtime, layer1_endpoint.clone(), arp, rng_seed)?,
            local_ipv4_addr: config.local_ipv4_addr()?,
        })
    }

    pub fn receive(
        &mut self,
        header: Ethernet2Header,
        packet: DemiBuffer,
    ) -> Result<Option<(Ipv4Header, DemiBuffer)>, Fail> {
        match header.ether_type() {
            EtherType2::Arp => {
                self.arp.receive(packet);
                Ok(None)
            },
            EtherType2::Ipv4 => {
                let (header, payload) = match Ipv4Header::parse(packet) {
                    Ok(result) => result,
                    Err(e) => {
                        let cause: String = format!("Invalid destination address: {:?}", e);
                        warn!("dropping packet: {}", cause);
                        return Err(Fail::new(libc::EINVAL, &cause));
                    },
                };
                debug!("Ipv4 received {:?}", header);
                if header.get_dest_addr() != self.local_ipv4_addr && !header.get_dest_addr().is_broadcast() {
                    let cause: String = format!("Invalid destination address");
                    warn!("dropping packet: {}", cause);
                    return Err(Fail::new(libc::EINVAL, &cause));
                }
                match header.get_protocol() {
                    IpProtocol::ICMPv4 => {
                        self.icmpv4.receive(header, payload);
                        Ok(None)
                    },
                    _ => Ok(Some((header, payload))),
                }
            },
            EtherType2::Ipv6 => Err(Fail::new(libc::ENOTSUP, "")), // Ignore for now.
        }
    }

    #[cfg(test)]
    pub fn get_local_addr(&self) -> Ipv4Addr {
        self.local_ipv4_addr
    }

    #[cfg(test)]
    pub async fn ping(&mut self, addr: Ipv4Addr, timeout: Option<Duration>) -> Result<Duration, Fail> {
        self.icmpv4.ping(addr, timeout).await
    }

    #[cfg(test)]
    pub async fn arp_query(&mut self, addr: Ipv4Addr) -> Result<MacAddress, Fail> {
        self.arp.query(addr).await
    }

    #[cfg(test)]
    pub fn export_arp_cache(&self) -> HashMap<Ipv4Addr, MacAddress, RandomState> {
        self.arp.export_cache()
    }
}
