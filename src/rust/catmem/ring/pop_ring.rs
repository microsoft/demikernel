// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::shared_ring::SharedRingBuffer,
    runtime::{
        fail::Fail,
        network::ring::{
            operation::RingControlOperation,
            state::RingStateMachine,
        },
    },
    scheduler::Mutex,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// An endpoint for a unidirectional queue built on a shared ring buffer
pub struct PopRing {
    /// Indicates whether the ring is open or closed.
    state_machine: RingStateMachine,
    /// Underlying buffer.
    buffer: SharedRingBuffer<u16>,
    /// Mutex to ensure single-threaded access to this ring.
    mutex: Mutex,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PopRing {
    /// Create a shared memory ring and connect to the pop/consumer-only end.
    pub fn create(name: &str, size: usize) -> Result<Self, Fail> {
        Ok(Self {
            state_machine: RingStateMachine::new(),
            buffer: SharedRingBuffer::create(name, size)?,
            mutex: Mutex::new(),
        })
    }

    /// Open an existing shared memory ring and connect to the pop/consumer-only end.
    pub fn open(name: &str, size: usize) -> Result<Self, Fail> {
        Ok(Self {
            state_machine: RingStateMachine::new(),
            buffer: SharedRingBuffer::open(name, size)?,
            mutex: Mutex::new(),
        })
    }

    /// Try to pop a byte from the shared memory ring. If successful, return the byte and whether the eof flag is set,
    /// otherwise return None for a retry.
    pub fn try_pop(&mut self) -> Result<(Option<u8>, bool), Fail> {
        if self.state_machine.is_closed() {
            return Err(Fail::new(libc::ECONNRESET, "queue was closed"));
        };
        if !self.mutex.try_lock() {
            let cause: String = format!("could not lock ring buffer to pop byte");
            warn!("try_pop(): {}", &cause);
            return Ok((None, false));
        }
        let out: Option<u16> = self.buffer.try_dequeue();
        assert_eq!(self.mutex.unlock().is_ok(), true);
        if let Some(bytes) = out {
            let (high, low): (u8, u8) = (((bytes >> 8) & 0xff) as u8, (bytes & 0xff) as u8);
            Ok((Some(low), high != 0))
        } else {
            Ok((None, false))
        }
    }

    /// Prepares a transition to the [PopRingState::Closing] state.
    pub fn prepare_close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(RingControlOperation::Close)
    }

    /// Prepares a transition to the [PopRingState::Closed] state.
    pub fn prepare_closed(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(RingControlOperation::Closed)
    }

    /// Commits to moving into the prepared state.
    pub fn commit(&mut self) {
        self.state_machine.commit();
    }
}
