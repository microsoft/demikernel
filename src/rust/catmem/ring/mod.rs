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

/// Maximum number of retries for pushing a EoF signal.
pub const MAX_RETRIES_PUSH_EOF: u32 = 16;

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

    /// This function commits the ring to closing.
    pub fn commit(&mut self) {
        match self {
            Ring::PushOnly(ring) => ring.commit(),
            Ring::PopOnly(ring) => ring.commit(),
        }
    }

    /// This function aborts a pending operation.
    pub fn abort(&mut self) {
        match self {
            Ring::PushOnly(ring) => ring.abort(),
            Ring::PopOnly(_) => warn!("abort() called on a pop-only queue"),
        }
    }

    /// Prepares a transition to the closing state.
    pub fn prepare_close(&mut self) -> Result<(), Fail> {
        match self {
            Ring::PushOnly(ring) => ring.prepare_close(),
            Ring::PopOnly(ring) => ring.prepare_close(),
        }
    }

    /// Prepares a transition to the closed state.
    pub fn prepare_closed(&mut self) -> Result<(), Fail> {
        match self {
            Ring::PushOnly(ring) => ring.prepare_closed(),
            Ring::PopOnly(ring) => ring.prepare_closed(),
        }
    }

    pub fn close(&mut self) -> Result<(), Fail> {
        match self {
            Ring::PushOnly(ring) => {
                // Attempt to push EoF.
                // Maximum number of retries. This is set to an arbitrary small value.
                for _ in 0..MAX_RETRIES_PUSH_EOF {
                    match ring.try_close() {
                        Ok(()) => return Ok(()),
                        Err(_) => continue,
                    }
                }
                let cause: String = format!("failed to push EoF");
                error!("push_eof(): {}", cause);
                Err(Fail::new(libc::EIO, &cause))
            },
            Ring::PopOnly(_) => Ok(()),
        }
    }

    pub fn try_close(&mut self) -> Result<(), Fail> {
        match self {
            Ring::PushOnly(ring) => ring.try_close(),
            Ring::PopOnly(_) => Ok(()),
        }
    }
}
