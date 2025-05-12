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
pub trait PhysicalLayer: 'static + DemiMemoryAllocator + Clone {
    /// State data required for managing flows.
    type FlowState: Default + Clone;

    type FlowRecord: Clone;

    /// Transmits a single [PacketBuf].
    fn transmit(&mut self, flow: &mut Self::FlowState, pkt: DemiBuffer) -> Result<(), Fail>;

    /// Receives a batch of [DemiBuffer].
    fn receive(&mut self) -> Result<ArrayVec<(Self::FlowRecord, DemiBuffer), RECEIVE_BATCH_SIZE>, Fail>;

    /// Update a FlowState from the FlowRecord received from the physical layer.
    fn update_flow_state(&mut self, _flow: &mut Self::FlowState, _record: Self::FlowRecord) {}

    /// Returns the ephemeral ports on which this physical layer may operate. If none, any valid ephemeral port may be used.
    fn ephemeral_ports(&self) -> EphemeralPorts {
        EphemeralPorts::default()
    }
}
