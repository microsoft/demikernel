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
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::{consts::RECEIVE_BATCH_SIZE, protocols::layer1::PhysicalLayer, types::MacAddress},
    runtime::{
        fail::Fail,
        memory::{DemiBuffer, MemoryRuntime},
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::ops::{Deref, DerefMut};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer2Endpoint<P: PhysicalLayer> {
    layer1_endpoint: P,
    local_link_addr: MacAddress,
}

#[derive(Clone)]
pub struct SharedLayer2Endpoint<P: PhysicalLayer>(SharedObject<Layer2Endpoint<P>>);

pub trait DataLinkLayer: 'static + Clone + Sized + MemoryRuntime {
    type PhysicalLayer: PhysicalLayer;
    type FlowState: Default;
    type FlowRecord: Clone;

    fn receive(&mut self) -> Result<ArrayVec<(EtherType2, Self::FlowRecord, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail>;

    fn transmit_arp_packet(
        &mut self,
        remote_link_addr: MacAddress,
        flow: &Self::FlowState,
        pkt: DemiBuffer,
    ) -> Result<(), Fail>;

    fn transmit_ipv4_packet(
        &mut self,
        remote_link_addr: MacAddress,
        flow: &Self::FlowState,
        pkt: DemiBuffer,
    ) -> Result<(), Fail>;

    fn get_local_link_addr(&self) -> MacAddress;

    fn update_flow_state(&mut self, flow: &mut Self::FlowState, record: Self::FlowRecord);
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<P: PhysicalLayer> SharedLayer2Endpoint<P> {
    pub fn new(config: &Config, layer1_endpoint: P) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(Layer2Endpoint {
            layer1_endpoint,
            local_link_addr: config.local_link_addr()?,
        })))
    }

    fn transmit(
        &mut self,
        remote_link_addr: MacAddress,
        eth2_type: EtherType2,
        flow: &P::FlowState,
        mut pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        let eth2_header: Ethernet2Header = Ethernet2Header::new(remote_link_addr, self.local_link_addr, eth2_type);
        debug!("L2 OUTGOING {:?}", eth2_header);
        eth2_header.serialize_and_attach(&mut pkt);
        self.layer1_endpoint.transmit(flow, pkt)
    }
}

impl<P: PhysicalLayer> DataLinkLayer for SharedLayer2Endpoint<P> {
    type PhysicalLayer = P;
    type FlowState = P::FlowState;
    type FlowRecord = P::FlowRecord;

    fn receive(&mut self) -> Result<ArrayVec<(EtherType2, P::FlowRecord, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail> {
        let mut batch: ArrayVec<(EtherType2, P::FlowRecord, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();
        for (flow_record, mut pkt) in self.layer1_endpoint.receive()? {
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
            batch.push((header.ether_type(), flow_record, pkt))
        }
        Ok(batch)
    }

    fn transmit_arp_packet(
        &mut self,
        remote_link_addr: MacAddress,
        flow: &Self::FlowState,
        pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        self.transmit(remote_link_addr, EtherType2::Arp, flow, pkt)
    }

    fn transmit_ipv4_packet(
        &mut self,
        remote_link_addr: MacAddress,
        flow: &Self::FlowState,
        pkt: DemiBuffer,
    ) -> Result<(), Fail> {
        self.transmit(remote_link_addr, EtherType2::Ipv4, flow, pkt)
    }

    fn get_local_link_addr(&self) -> MacAddress {
        self.local_link_addr
    }

    fn update_flow_state(&mut self, flow: &mut P::FlowState, record: P::FlowRecord) {
        self.layer1_endpoint.update_flow_state(flow, record)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<P: PhysicalLayer> Deref for SharedLayer2Endpoint<P> {
    type Target = Layer2Endpoint<P>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<P: PhysicalLayer> DerefMut for SharedLayer2Endpoint<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Memory Runtime Trait Implementation for the network stack.
impl<P: PhysicalLayer> MemoryRuntime for SharedLayer2Endpoint<P> {
    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer1_endpoint.into_sgarray(buf)
    }

    fn sgaalloc(&self, size_bytes: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer1_endpoint.sgaalloc(size_bytes)
    }

    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer1_endpoint.sgafree(sga)
    }

    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer1_endpoint.clone_sgarray(sga)
    }
}
