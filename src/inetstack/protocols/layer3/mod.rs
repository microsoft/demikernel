// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

pub mod arp;
pub mod icmpv4;
pub mod ip;
pub mod ipv4;
pub use self::{arp::SharedArpPeer, icmpv4::SharedIcmpv4Peer, ip::IpProtocol, ipv4::Ipv4Header};
use crate::{
    demikernel::config::Config,
    inetstack::{
        consts::MAX_BATCH_SIZE_NUM_PACKETS,
        protocols::layer2::{EtherType2, SharedLayer2Endpoint},
    },
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, DemiMemoryAllocator},
        SharedDemiRuntime, SharedObject,
    },
    MacAddress,
};
use ::arrayvec::ArrayVec;
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
    local_ip: Ipv4Addr,
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
        let arp = SharedArpPeer::new(config, runtime.clone(), layer2_endpoint.clone())?;

        Ok(SharedLayer3Endpoint(SharedObject::new(Layer3Endpoint {
            arp: arp.clone(),
            icmpv4: SharedIcmpv4Peer::new(config, runtime, layer2_endpoint.clone(), arp, rng_seed)?,
            local_ip: config.local_ipv4_addr()?,
            layer2_endpoint,
        })))
    }

    pub fn receive(
        &mut self,
    ) -> Result<ArrayVec<(Ipv4Addr, IpProtocol, DemiBuffer), MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        let mut batch = ArrayVec::new();
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
                            warn!("dropping packet: Invalid destination address: {:?}", e);
                            continue;
                        },
                    };
                    debug!("L3 INCOMING {:?}", header);

                    if !self.is_for_us(header) {
                        warn!("dropping packet: Invalid destination address");
                        continue;
                    }

                    if bad_src(header) {
                        warn!("dropping packet: Invalid source addr ({})", header.src_addr());
                        continue;
                    }

                    let protocol = header.protocol();
                    match protocol {
                        IpProtocol::ICMPv4 => {
                            self.icmpv4.receive(header, packet);
                            continue;
                        },
                        _ => batch.push((header.src_addr(), protocol, packet)),
                    }
                },
                EtherType2::Ipv6 => warn!("Ipv6 not supported yet"), // Ignore for now.
            }
        }
        Ok(batch)
    }

    fn is_for_us(&mut self, header: Ipv4Header) -> bool {
        let dst = header.dst_addr();
        dst == self.local_ip || dst.is_broadcast()
    }

    pub fn transmit_tcp_packet_nonblocking(&mut self, remote_ip: Ipv4Addr, pkt: DemiBuffer) -> Result<(), Fail> {
        let remote_mac = match self.arp.try_query(remote_ip) {
            Some(mac) => mac,
            _ => return Err(Fail::new(libc::EAGAIN, "destination not in ARP cache")),
        };

        self.transmit_packet(remote_ip, remote_mac, IpProtocol::TCP, pkt)
    }

    pub async fn transmit_tcp_packet_blocking(&mut self, remote_ip: Ipv4Addr, pkt: DemiBuffer) -> Result<(), Fail> {
        let remote_mac = self.arp.query(remote_ip).await?;
        self.transmit_packet(remote_ip, remote_mac, IpProtocol::TCP, pkt)
    }

    pub async fn transmit_udp_packet_blocking(&mut self, remote_ip: Ipv4Addr, pkt: DemiBuffer) -> Result<(), Fail> {
        let remote_mac = self.arp.query(remote_ip).await?;
        self.transmit_packet(remote_ip, remote_mac, IpProtocol::UDP, pkt)
    }

    pub fn transmit_packet(
        &mut self,
        remote_ip: Ipv4Addr,
        remote_mac: MacAddress,
        ip_protocol: IpProtocol,
        mut pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        let header = Ipv4Header::new(self.local_ip, remote_ip, ip_protocol);
        debug!("L3 OUTGOING {:?}", header);
        header.serialize_and_attach(&mut pkt);
        self.layer2_endpoint.transmit_ipv4_packet(remote_mac, pkt)
    }

    pub fn get_local_addr(&self) -> Ipv4Addr {
        self.local_ip
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

fn bad_src(hdr: Ipv4Header) -> bool {
    let src = hdr.src_addr();
    src.is_broadcast() || src.is_multicast() || src.is_unspecified()
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
    fn max_buffer_size_bytes(&self) -> usize {
        self.layer2_endpoint.max_buffer_size_bytes()
    }

    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        self.layer2_endpoint.allocate_demi_buffer(size)
    }
}
