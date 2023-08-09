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
// Constants
//======================================================================================================================

/// End of file signal.
const EOF: u16 = (1 & 0xff) << 8;

//======================================================================================================================
// Structures
//======================================================================================================================

/// An endpoint for a unidirectional queue built on a shared ring buffer
pub struct PushRing {
    /// State machine.
    state_machine: RingStateMachine,
    /// Underlying buffer.
    buffer: SharedRingBuffer<u16>,
    /// Serialize access to underlying ring buffer.
    mutex: Mutex,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl PushRing {
    /// Creates a new shared memory ring and connects to the push/producer-only end.
    pub fn create(name: &str, size: usize) -> Result<Self, Fail> {
        Ok(Self {
            state_machine: RingStateMachine::new(),
            buffer: SharedRingBuffer::create(name, size)?,
            mutex: Mutex::new(),
        })
    }

    /// Opens an existing shared memory ring and connects to the push/producer-only end.
    pub fn open(name: &str, size: usize) -> Result<Self, Fail> {
        Ok(Self {
            state_machine: RingStateMachine::new(),
            buffer: SharedRingBuffer::open(name, size)?,
            mutex: Mutex::new(),
        })
    }

    /// Prepares a transition to the [PushRingState::Closing] state.
    pub fn prepare_close(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(RingControlOperation::Close)
    }

    /// Prepares a transition to the [PushRingState::Closed] state.
    pub fn prepare_closed(&mut self) -> Result<(), Fail> {
        self.state_machine.prepare(RingControlOperation::Closed)
    }

    /// Try to send an eof through the shared memory ring. If success, this queue is now closed, otherwise, return
    /// EAGAIN and retry.
    pub fn try_close(&mut self) -> Result<(), Fail> {
        // Try to lock the ring buffer.
        if !self.mutex.try_lock() {
            let cause: String = format!("could not lock ring buffer to push EOF");
            error!("try_close(): {}", &cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        let result: Result<(), u16> = self.buffer.try_enqueue(EOF);
        assert_eq!(self.mutex.unlock().is_ok(), true);

        match result {
            Ok(()) => Ok(()),
            Err(_) => Err(Fail::new(libc::EAGAIN, "Could not push EOF")),
        }
    }

    /// Try to send a byte through the shared memory ring. If there is no space or another thread is writing to this
    /// ring, return [false], otherwise, return [true] if successfully enqueued.
    pub fn try_push(&mut self, byte: &u8) -> Result<bool, Fail> {
        let x: u16 = (*byte & 0xff) as u16;
        // Try to lock the ring buffer.
        if !self.mutex.try_lock() {
            let cause: String = format!("could not lock ring buffer to push byte");
            warn!("try_push(): {}", &cause);
            return Ok(false);
        }
        // Write to the ring buffer.
        let result: Result<(), u16> = self.buffer.try_enqueue(x);
        // Unlock the ring buffer.
        assert_eq!(self.mutex.unlock().is_ok(), true);
        // Return result.
        Ok(result.is_ok())
    }

    /// Commits to moving into the prepared state.
    pub fn commit(&mut self) {
        self.state_machine.commit();
    }

    /// Aborts prepared state.
    pub fn abort(&mut self) {
        self.state_machine.abort();
    }
}
