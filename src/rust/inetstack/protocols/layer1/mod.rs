// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod packet;
pub use ::std::any::Any;
use arrayvec::ArrayVec;
pub use packet::PacketBuf;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::{
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::consts::RECEIVE_BATCH_SIZE,
    },
    MacAddress,
};

//======================================================================================================================
// Traits
//======================================================================================================================

/// Network Runtime
pub trait PhysicalLayer: MemoryRuntime + Any {
    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: &dyn PacketBuf);

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>;

    /// Get physical address.
    fn get_link_addr(&self) -> MacAddress;

    /// Downcast for testing. NOT marked for conditional compile because not all of our tests run in test mode.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
