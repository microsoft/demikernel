// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::layer4::ephemeral::EphemeralPorts;
use crate::runtime::memory::DemiMemoryAllocator;
use crate::runtime::{fail::Fail, memory::DemiBuffer};
pub use ::std::any::Any;

//======================================================================================================================
// Traits
//======================================================================================================================

/// API for the Physical Layer for any underlying hardware that implements a raw NIC interface (e.g., DPDK, raw
/// sockets). It must implement [DemiMemoryAllocator] to specify how to allocate DemiBuffers for the physical layer.
pub trait PhysicalLayer: 'static + DemiMemoryAllocator {
    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail>;

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> Result<Vec<DemiBuffer>, Fail>;

    /// Returns the ephemeral ports on which this physical layer may operate. If none, any valid ephemeral port may be used.
    fn ephemeral_ports(&self) -> EphemeralPorts {
        EphemeralPorts::default()
    }
}
