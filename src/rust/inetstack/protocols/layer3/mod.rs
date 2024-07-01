// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

pub mod arp;
pub mod icmpv4;
pub mod ip;
pub mod ipv4;

use arrayvec::ArrayVec;

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
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::protocols::{
        layer1::PacketBuf,
        layer2::{
            EtherType2,
            SharedLayer2Endpoint,
        },
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::consts::RECEIVE_BATCH_SIZE,
        SharedDemiRuntime,
        SharedObject,
    },
};
#[cfg(test)]
use ::std::{
    collections::HashMap,
    hash::RandomState,
};
use ::std::{
    net::Ipv4Addr,
    ops::{
        Deref,
        DerefMut,
    },
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer3Endpoint {
    layer2_endpoint: SharedLayer2Endpoint,
    // TODO: Remove this once we have proper separation between layers.
    arp: SharedArpPeer,
    icmpv4: SharedIcmpv4Peer,
    local_ipv4_addr: Ipv4Addr,
}

#[derive(Clone)]
pub struct SharedLayer3Endpoint(SharedObject<Layer3Endpoint>);

//======================================================================================================================
// Associated Functions

//======================================================================================================================

impl SharedLayer3Endpoint {
    pub fn new(
        config: &Config,
        runtime: SharedDemiRuntime,
        arp: SharedArpPeer,
        layer2_endpoint: SharedLayer2Endpoint,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        Ok(SharedLayer3Endpoint(SharedObject::new(Layer3Endpoint {
            arp: arp.clone(),
            icmpv4: SharedIcmpv4Peer::new(&config, runtime, layer2_endpoint.clone(), arp, rng_seed)?,
            local_ipv4_addr: config.local_ipv4_addr()?,
            layer2_endpoint,
        })))
    }

    pub fn receive(&mut self) -> ArrayVec<(Ipv4Header, DemiBuffer), RECEIVE_BATCH_SIZE> {
        let mut batch: ArrayVec<(Ipv4Header, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();
        for (header, packet) in self.layer2_endpoint.receive() {
            match header.ether_type() {
                EtherType2::Arp => {
                    self.arp.receive(packet);
                    continue;
                },
                EtherType2::Ipv4 => {
                    let (header, payload) = match Ipv4Header::parse(packet) {
                        Ok(result) => result,
                        Err(e) => {
                            let cause: String = format!("Invalid destination address: {:?}", e);
                            warn!("dropping packet: {}", cause);
                            continue;
                        },
                    };
                    debug!("Ipv4 received {:?}", header);
                    if header.get_dest_addr() != self.local_ipv4_addr && !header.get_dest_addr().is_broadcast() {
                        let cause: String = format!("Invalid destination address");
                        warn!("dropping packet: {}", cause);
                        continue;
                    }
                    match header.get_protocol() {
                        IpProtocol::ICMPv4 => {
                            self.icmpv4.receive(header, payload);
                            continue;
                        },
                        _ => batch.push((header, payload)),
                    }
                },
                EtherType2::Ipv6 => continue, // Ignore for now.
            }
        }
        batch
    }

    pub fn transmit<P: PacketBuf>(&mut self, pkt: &P) {
        self.layer2_endpoint.transmit(pkt)
    }

    #[cfg(test)]
    pub fn get_local_addr(&self) -> Ipv4Addr {
        self.local_ipv4_addr
    }

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

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedLayer3Endpoint {
    type Target = Layer3Endpoint;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedLayer3Endpoint {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Memory Runtime Trait Implementation for IP Layer.
impl MemoryRuntime for Layer3Endpoint {
    /// Casts a [DPDKBuf] into an [demi_sgarray_t].
    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer2_endpoint.into_sgarray(buf)
    }

    /// Allocates a [demi_sgarray_t].
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer2_endpoint.sgaalloc(size)
    }

    /// Releases a [demi_sgarray_t].
    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer2_endpoint.sgafree(sga)
    }

    /// Clones a [demi_sgarray_t].
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer2_endpoint.clone_sgarray(sga)
    }
}
