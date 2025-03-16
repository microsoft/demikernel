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

use crate::inetstack::consts::RECEIVE_BATCH_SIZE;
use crate::runtime::{
    fail::Fail,
    memory::{DemiBuffer, MemoryRuntime},
};

use super::layer4::ephemeral::EphemeralPorts;

//======================================================================================================================
// Traits
//======================================================================================================================

/// API for the Physical Layer for any underlying hardware that implements a raw NIC interface (e.g., DPDK, raw
/// sockets).
pub trait PhysicalLayer: 'static + MemoryRuntime {
    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail>;

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail>;

    /// Returns the ephemeral ports on which this physical layer may operate. If none, any valid ephemeral port may be used.
    fn ephemeral_ports(&self) -> EphemeralPorts {
        EphemeralPorts::default()
    }
}
