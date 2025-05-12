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

pub use self::{arp::SharedArpPeer, icmpv4::SharedIcmpv4Peer, ip::IpProtocol, ipv4::Ipv4Header};

use crate::{
    demikernel::config::Config,
    inetstack::{
        consts::RECEIVE_BATCH_SIZE,
        protocols::layer2::{EtherType2, SharedLayer2Endpoint},
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, DemiMemoryAllocator},
        SharedDemiRuntime, SharedObject,
    },
    MacAddress,
};
#[cfg(test)]
use ::std::{collections::HashMap, hash::RandomState, time::Duration};
use ::std::{
    net::Ipv4Addr,
    ops::{Deref, DerefMut},
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer3Endpoint {
    layer2_endpoint: SharedLayer2Endpoint,
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
        layer2_endpoint: SharedLayer2Endpoint,
        rng_seed: [u8; 32],
    ) -> Result<Self, Fail> {
        let arp: SharedArpPeer = SharedArpPeer::new(config, runtime.clone(), layer2_endpoint.clone())?;

        Ok(SharedLayer3Endpoint(SharedObject::new(Layer3Endpoint {
            arp: arp.clone(),
            icmpv4: SharedIcmpv4Peer::new(&config, runtime, layer2_endpoint.clone(), arp, rng_seed)?,
            local_ipv4_addr: config.local_ipv4_addr()?,
            layer2_endpoint,
        })))
    }

    pub fn receive(&mut self) -> Result<ArrayVec<(Ipv4Addr, IpProtocol, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail> {
        let mut batch: ArrayVec<(Ipv4Addr, IpProtocol, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();
        for (eth2_type, mut packet) in self.layer2_endpoint.receive()? {
            match eth2_type {
                EtherType2::Arp => {
                    self.arp.receive(packet);
                    continue;
                },
                EtherType2::Ipv4 => {
                    let header = match Ipv4Header::parse_and_strip(&mut packet) {
                        Ok(header) => header,
                        Err(e) => {
                            let cause: String = format!("Invalid destination address: {:?}", e);
                            warn!("dropping packet: {}", cause);
                            continue;
                        },
                    };
                    debug!("L3 INCOMING {:?}", header);

                    // Check that the destination matches our IP address; otherwise, discard.
                    if header.get_dest_addr() != self.local_ipv4_addr && !header.get_dest_addr().is_broadcast() {
                        let cause: String = format!("Invalid destination address");
                        warn!("dropping packet: {}", cause);
                        continue;
                    }

                    // Check the the source is a valid IP address; otherwise, discard.
                    if header.get_src_addr().is_broadcast()
                        || header.get_src_addr().is_multicast()
                        || header.get_src_addr().is_unspecified()
                    {
                        let cause: String = format!("invalid remote address (remote={})", header.get_src_addr());
                        warn!("dropping packet: {}", &cause);
                        continue;
                    }

                    let protocol: IpProtocol = header.get_protocol();
                    match protocol {
                        IpProtocol::ICMPv4 => {
                            self.icmpv4.receive(header, packet);
                            continue;
                        },
                        _ => batch.push((header.get_src_addr(), protocol, packet)),
                    }
                },
                EtherType2::Ipv6 => warn!("Ipv6 not supported yet"), // Ignore for now.
            }
        }
        Ok(batch)
    }

    pub fn transmit_tcp_packet_nonblocking(&mut self, remote_ipv4_addr: Ipv4Addr, pkt: DemiBuffer) -> Result<(), Fail> {
        let remote_link_addr: MacAddress = match self.arp.try_query(remote_ipv4_addr) {
            Some(addr) => addr,
            _ => return Err(Fail::new(libc::EAGAIN, "destination not in ARP cache")),
        };

        self.transmit_packet(remote_ipv4_addr, remote_link_addr, IpProtocol::TCP, pkt)
    }

    pub async fn transmit_tcp_packet_blocking(
        &mut self,
        remote_ipv4_addr: Ipv4Addr,
        pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        let remote_link_addr: MacAddress = self.arp.query(remote_ipv4_addr).await?;

        self.transmit_packet(remote_ipv4_addr, remote_link_addr, IpProtocol::TCP, pkt)
    }

    pub async fn transmit_udp_packet_blocking(
        &mut self,
        remote_ipv4_addr: Ipv4Addr,
        pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        let remote_link_addr: MacAddress = self.arp.query(remote_ipv4_addr).await?;

        self.transmit_packet(remote_ipv4_addr, remote_link_addr, IpProtocol::UDP, pkt)
    }

    pub fn transmit_packet(
        &mut self,
        remote_ipv4_addr: Ipv4Addr,
        remote_link_addr: MacAddress,
        ip_protocol: IpProtocol,
        mut pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        let ipv4_header: Ipv4Header = Ipv4Header::new(self.local_ipv4_addr, remote_ipv4_addr, ip_protocol);
        debug!("L3 OUTGOING {:?}", ipv4_header);
        ipv4_header.serialize_and_attach(&mut pkt);
        self.layer2_endpoint.transmit_ipv4_packet(remote_link_addr, pkt)
    }

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

/// Memory Runtime Trait Implementation for Layer 3.
impl DemiMemoryAllocator for SharedLayer3Endpoint {
    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        self.layer2_endpoint.allocate_demi_buffer(size)
    }
}
