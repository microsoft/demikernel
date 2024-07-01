// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod ethernet2;
pub use self::ethernet2::{
    frame::{
        Ethernet2Header,
        ETHERNET2_HEADER_SIZE,
        MIN_PAYLOAD_SIZE,
    },
    protocol::EtherType2,
};

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demi_sgarray_t,
    demikernel::config::Config,
    inetstack::protocols::layer1::{
        PacketBuf,
        PhysicalLayer,
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::consts::RECEIVE_BATCH_SIZE,
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
#[cfg(test)]
use ::std::any::Any;
use ::std::ops::{
    Deref,
    DerefMut,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer2Endpoint {
    layer1_endpoint: Box<dyn PhysicalLayer>,
}

#[derive(Clone)]
pub struct SharedLayer2Endpoint(SharedObject<Layer2Endpoint>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedLayer2Endpoint {
    pub fn new(_config: &Config, layer1_endpoint: Box<dyn PhysicalLayer>) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(Layer2Endpoint { layer1_endpoint })))
    }

    pub fn receive(&mut self) -> ArrayVec<(Ethernet2Header, DemiBuffer), RECEIVE_BATCH_SIZE> {
        let mut batch: ArrayVec<(Ethernet2Header, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();
        for pkt in self.layer1_endpoint.receive() {
            let (header, payload) = match Ethernet2Header::parse(pkt) {
                Ok(result) => result,
                Err(e) => {
                    // TODO: Collect dropped packet statistics.
                    let cause: &str = "Invalid Ethernet header";
                    warn!("{}: {:?}", cause, e);
                    continue;
                },
            };
            debug!("Engine received {:?}", header);
            if self.layer1_endpoint.get_link_addr() != header.dst_addr()
                && !header.dst_addr().is_broadcast()
                && !header.dst_addr().is_multicast()
            {
                let cause: &str = "invalid link address";
                warn!("dropping packet: {}", cause);
            }
            batch.push((header, payload))
        }
        batch
    }

    pub fn transmit<P: PacketBuf>(&mut self, pkt: &P) {
        // TODO: Remove box later if possible.
        self.layer1_endpoint.transmit(pkt)
    }

    #[cfg(test)]
    pub fn get_physical_layer<P: PhysicalLayer>(&mut self) -> &mut P {
        // 1. Get reference to queue inside the box.
        let layer1_ptr: &mut dyn PhysicalLayer = self.layer1_endpoint.as_mut();
        // 2. Cast that reference to a void pointer for downcasting.
        let void_ptr: &mut dyn Any = layer1_ptr.as_any_mut();
        // 3. Downcast to concrete type T
        void_ptr.downcast_mut::<P>().expect("This should only be a test engine")
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

/// Memory Runtime Trait Implementation for the network stack.
impl MemoryRuntime for Layer2Endpoint {
    /// Casts a [DPDKBuf] into an [demi_sgarray_t].
    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        self.layer1_endpoint.into_sgarray(buf)
    }

    /// Allocates a [demi_sgarray_t].
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.layer1_endpoint.sgaalloc(size)
    }

    /// Releases a [demi_sgarray_t].
    fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.layer1_endpoint.sgafree(sga)
    }

    /// Clones a [demi_sgarray_t].
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.layer1_endpoint.clone_sgarray(sga)
    }
}
