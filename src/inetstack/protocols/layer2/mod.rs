// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod ethernet2;
pub use self::ethernet2::{
    header::{Ethernet2Header, ETHERNET2_HEADER_SIZE, MIN_PAYLOAD_SIZE},
    protocol::EtherType2,
};

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    inetstack::{consts::MAX_BATCH_SIZE_NUM_PACKETS, protocols::layer1::PhysicalLayer, types::MacAddress},
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, DemiMemoryAllocator},
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::ops::{Deref, DerefMut};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer2Endpoint {
    layer1_endpoint: Box<dyn PhysicalLayer>,
    local_mac: MacAddress,
}

#[derive(Clone)]
pub struct SharedLayer2Endpoint(SharedObject<Layer2Endpoint>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedLayer2Endpoint {
    pub fn new<P: PhysicalLayer>(config: &Config, layer1_endpoint: P) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(Layer2Endpoint {
            layer1_endpoint: Box::new(layer1_endpoint),
            local_mac: config.local_link_addr()?,
        })))
    }

    pub fn receive(&mut self) -> Result<ArrayVec<(EtherType2, DemiBuffer), MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        let mut batch = ArrayVec::new();
        for mut packet in self.layer1_endpoint.receive()? {
            let header = match Ethernet2Header::parse_and_strip(&mut packet) {
                Ok(result) => result,
                Err(e) => {
                    // TODO: Collect dropped packet statistics.
                    warn!("Invalid Ethernet header: {:?}", e);
                    continue;
                },
            };
            debug!("L2 INCOMING {:?}", header);
            if self.bad_dst(&header) {
                warn!("dropping packet: invalid link address");
            }
            batch.push((header.ether_type(), packet))
        }
        Ok(batch)
    }

    pub fn transmit_arp_packet(&mut self, remote_mac: MacAddress, packet: DemiBuffer) -> Result<(), Fail> {
        let mut packets = ArrayVec::new();
        packets.push(packet);
        self.transmit(remote_mac, EtherType2::Arp, packets)
    }

    pub fn transmit_ipv4_packet(&mut self, remote_mac: MacAddress, packet: DemiBuffer) -> Result<(), Fail> {
        let mut packets = ArrayVec::new();
        packets.push(packet);
        self.transmit(remote_mac, EtherType2::Ipv4, packets)
    }

    fn bad_dst(&self, header: &Ethernet2Header) -> bool {
        let dst_mac = header.dst_addr();
        self.local_mac != dst_mac && !dst_mac.is_broadcast() && !dst_mac.is_multicast()
    }

    fn transmit(
        &mut self,
        remote_mac: MacAddress,
        ether_type: EtherType2,
        mut packets: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>,
    ) -> Result<(), Fail> {
        let header = Ethernet2Header::new(remote_mac, self.local_mac, ether_type);

        debug!("L2 OUTGOING {:?}", header);

        for packet in packets.iter_mut() {
            header.serialize_and_attach(packet);
        }

        self.layer1_endpoint.transmit(packets)
    }

    pub fn get_local_link_addr(&self) -> MacAddress {
        self.local_mac
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedLayer2Endpoint {
    type Target = Layer2Endpoint;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedLayer2Endpoint {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl DemiMemoryAllocator for SharedLayer2Endpoint {
    fn max_buffer_size_bytes(&self) -> usize {
        self.layer1_endpoint.max_buffer_size_bytes()
    }

    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        self.layer1_endpoint.allocate_demi_buffer(size)
    }
}
