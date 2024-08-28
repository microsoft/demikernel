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
        network::{
            consts::RECEIVE_BATCH_SIZE,
            types::MacAddress,
        },
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::ops::{
    Deref,
    DerefMut,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Layer2Endpoint {
    layer1_endpoint: Box<dyn PhysicalLayer>,
    link_addr: MacAddress,
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
            link_addr: config.local_link_addr()?,
        })))
    }

    pub fn receive(&mut self) -> Result<ArrayVec<(Ethernet2Header, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail> {
        let mut batch: ArrayVec<(Ethernet2Header, DemiBuffer), RECEIVE_BATCH_SIZE> = ArrayVec::new();
        for pkt in self.layer1_endpoint.receive()? {
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
            if self.link_addr != header.dst_addr()
                && !header.dst_addr().is_broadcast()
                && !header.dst_addr().is_multicast()
            {
                let cause: &str = "invalid link address";
                warn!("dropping packet: {}", cause);
            }
            batch.push((header, payload))
        }
        Ok(batch)
    }

    pub fn transmit<P: PacketBuf>(&mut self, mut pkt: P) -> Result<(), Fail> {
        let buf: DemiBuffer = match pkt.take_body() {
            Some(body) => body,
            _ => {
                let cause = format!("No body in PacketBuf to transmit");
                warn!("{}", cause);
                return Err(Fail::new(libc::EINVAL, &cause));
            },
        };
        self.layer1_endpoint.transmit(buf)
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
