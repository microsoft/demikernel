// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub use ::std::any::Any;
use arrayvec::ArrayVec;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    memory::{
        DemiBuffer,
        MemoryRuntime,
    },
    network::consts::RECEIVE_BATCH_SIZE,
};

use crate::autokernel::parameters::{MAX_RECEIVE_BATCH_SIZE};

//======================================================================================================================
// Traits
//======================================================================================================================

/// API for the Physical Layer for any underlying hardware that implements a raw NIC interface (e.g., DPDK, raw
/// sockets).
pub trait PhysicalLayer: 'static + MemoryRuntime {
    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail>;

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, MAX_RECEIVE_BATCH_SIZE>, Fail>;
}
