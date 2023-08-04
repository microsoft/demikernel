// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

mod pop_ring;
mod push_ring;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    pop_ring::PopRing,
    push_ring::PushRing,
};
use crate::runtime::fail::Fail;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Capacity of the ring buffer, in bytes.
/// This does not correspond to the effective number of bytes that may be stored in the ring buffer due to layout and
/// padding. Still, this is intentionally set so as the effective capacity is large enough to hold 16 KB of data.
const RING_BUFFER_CAPACITY: usize = 65536;

//======================================================================================================================
// Structures
//======================================================================================================================

/// An endpoint for a unidirectional queue built on a shared ring buffer
pub enum Ring {
    PushOnly(PushRing),
    PopOnly(PopRing),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Ring {
    /// Creates a new shared memory ring and connects to the push/producer end.
    pub fn create_pop_ring(name: &str) -> Result<Self, Fail> {
        Ok(Self::PopOnly(PopRing::create(name, RING_BUFFER_CAPACITY)?))
    }

    /// Creates a new shared memory ring and connects to the push/producer end.
    pub fn create_push_ring(name: &str) -> Result<Self, Fail> {
        Ok(Self::PushOnly(PushRing::create(name, RING_BUFFER_CAPACITY)?))
    }

    /// Opens an existing shared memory ring and connects to the pop/consumer end.
    pub fn open_pop_ring(name: &str) -> Result<Self, Fail> {
        Ok(Self::PopOnly(PopRing::open(name, RING_BUFFER_CAPACITY)?))
    }

    /// Opens an existing shared memory ring and connects to the push/producer end.
    pub fn open_push_ring(name: &str) -> Result<Self, Fail> {
        Ok(Self::PushOnly(PushRing::open(name, RING_BUFFER_CAPACITY)?))
    }
}
