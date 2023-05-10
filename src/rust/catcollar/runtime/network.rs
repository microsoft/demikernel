// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::IoUringRuntime;
use crate::runtime::{
    memory::DemiBuffer,
    network::{
        NetworkRuntime,
        PacketBuf,
    },
};
use ::arrayvec::ArrayVec;

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for I/O User Ring Runtime
impl<const N: usize> NetworkRuntime<N> for IoUringRuntime {
    // TODO: Rely on a default implementation for this.
    fn transmit(&self, _pkt: Box<dyn PacketBuf>) {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn receive(&self) -> ArrayVec<DemiBuffer, N> {
        unreachable!()
    }
}
