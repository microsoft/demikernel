// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{fail::Fail, memory::DemiBuffer};
use crate::{inetstack::consts::RECEIVE_BATCH_SIZE, runtime::memory::DemiMemoryAllocator};
pub use ::std::any::Any;
use arrayvec::ArrayVec;

use super::layer4::ephemeral::EphemeralPorts;

//======================================================================================================================
// Traits
//======================================================================================================================

/// API for the Physical Layer for any underlying hardware that implements a raw NIC interface (e.g., DPDK, raw
/// sockets). It must implement [DemiMemoryAllocator] to specify how to allocate DemiBuffers for the physical layer.
pub trait PhysicalLayer: 'static + DemiMemoryAllocator {
    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail>;

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail>;

    /// Returns the ephemeral ports on which this physical layer may operate. If none, any valid ephemeral port may be used.
    fn ephemeral_ports(&self) -> EphemeralPorts {
        EphemeralPorts::default()
    }
}
