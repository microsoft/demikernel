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
    inetstack::{consts::RECEIVE_BATCH_SIZE, protocols::layer1::PhysicalLayer, types::MacAddress},
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
    local_link_addr: MacAddress,
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
            local_link_addr: config.local_link_addr()?,
        })))
    }

    pub fn receive(&mut self) -> Result<ArrayVec<(EtherType2, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail> {
        let mut batch: ArrayVec<(EtherType2, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();
        for mut pkt in self.layer1_endpoint.receive()? {
            let header: Ethernet2Header = match Ethernet2Header::parse_and_strip(&mut pkt) {
                Ok(result) => result,
                Err(e) => {
                    // TODO: Collect dropped packet statistics.
                    let cause: &str = "Invalid Ethernet header";
                    warn!("{}: {:?}", cause, e);
                    continue;
                },
            };
            debug!("L2 INCOMING {:?}", header);
            if self.local_link_addr != header.dst_addr()
                && !header.dst_addr().is_broadcast()
                && !header.dst_addr().is_multicast()
            {
                let cause: &str = "invalid link address";
                warn!("dropping packet: {}", cause);
            }
            batch.push((header.ether_type(), pkt))
        }
        Ok(batch)
    }

    pub fn transmit_arp_packet(&mut self, remote_link_addr: MacAddress, pkt: DemiBuffer) -> Result<(), Fail> {
        self.transmit(remote_link_addr, EtherType2::Arp, pkt)
    }

    pub fn transmit_ipv4_packet(&mut self, remote_link_addr: MacAddress, pkt: DemiBuffer) -> Result<(), Fail> {
        self.transmit(remote_link_addr, EtherType2::Ipv4, pkt)
    }

    fn transmit(
        &mut self,
        remote_link_addr: MacAddress,
        eth2_type: EtherType2,
        mut pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        let eth2_header: Ethernet2Header = Ethernet2Header::new(remote_link_addr, self.local_link_addr, eth2_type);
        debug!("L2 OUTGOING {:?}", eth2_header);
        eth2_header.serialize_and_attach(&mut pkt);
        self.layer1_endpoint.transmit(pkt)
    }

    pub fn get_local_link_addr(&self) -> MacAddress {
        self.local_link_addr
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
    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        self.layer1_endpoint.allocate_demi_buffer(size)
    }
}
