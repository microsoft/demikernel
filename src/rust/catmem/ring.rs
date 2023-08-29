// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::{
        ring::RingBuffer,
        shared_ring::SharedRingBuffer,
    },
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
pub struct Ring {
    /// Underlying buffer used for sending data.
    push_buf: SharedRingBuffer<RingBuffer<u16>>,
    /// Underlying buffer used for receiving data.
    pop_buf: SharedRingBuffer<RingBuffer<u16>>,
    /// Indicates whether the ring is open or closed.
    state_machine: RingStateMachine,
    /// Mutex to ensure single-threaded access to this ring.
    mutex: Mutex,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Ring {
    /// Creates a new shared memory ring.
    pub fn create(name: &str) -> Result<Self, Fail> {
        // Check if provided name is valid.
        if name.is_empty() {
            return Err(Fail::new(libc::EINVAL, "name of shared memory region cannot be empty"));
        }
        Ok(Self {
            push_buf: SharedRingBuffer::create(&format!("{}:tx", name), RING_BUFFER_CAPACITY)?,
            pop_buf: SharedRingBuffer::create(&format!("{}:rx", name), RING_BUFFER_CAPACITY)?,
            state_machine: RingStateMachine::new(),
            mutex: Mutex::new(),
        })
    }

    /// Opens an existing shared memory ring.
    pub fn open(name: &str) -> Result<Self, Fail> {
        // Check if provided name is valid.
        if name.is_empty() {
            return Err(Fail::new(libc::EINVAL, "name of shared memory region cannot be empty"));
        }
        Ok(Self {
            push_buf: SharedRingBuffer::open(&format!("{}:rx", name), RING_BUFFER_CAPACITY)?,
            pop_buf: SharedRingBuffer::open(&format!("{}:tx", name), RING_BUFFER_CAPACITY)?,
            state_machine: RingStateMachine::new(),
            mutex: Mutex::new(),
        })
    }

    /// Try to pop a byte from the shared memory ring. If successful, return the byte and whether the eof flag is set,
    /// otherwise return None for a retry.
    pub fn try_pop(&mut self) -> Result<(Option<u8>, bool), Fail> {
        if self.state_machine.is_closing() {
            return Err(Fail::new(libc::ECANCELED, "queue is closing"));
        } else if self.state_machine.is_closed() {
            return Err(Fail::new(libc::ECANCELED, "queue was closed"));
        };
        if !self.mutex.try_lock() {
            let cause: String = format!("could not lock ring buffer to pop byte");
            warn!("try_pop(): {}", &cause);
            return Ok((None, false));
        }
        let out: Option<u16> = self.pop_buf.try_dequeue();
        assert_eq!(self.mutex.unlock().is_ok(), true);
        if let Some(bytes) = out {
            let (high, low): (u8, u8) = (((bytes >> 8) & 0xff) as u8, (bytes & 0xff) as u8);
            Ok((Some(low), high != 0))
        } else {
            Ok((None, false))
        }
    }

    /// Try to send a byte through the shared memory ring. If there is no space or another thread is writing to this
    /// ring, return [false], otherwise, return [true] if successfully enqueued.
    pub fn try_push(&mut self, byte: &u8) -> Result<bool, Fail> {
        self.state_machine.may_push()?;

        let x: u16 = (*byte & 0xff) as u16;
        // Try to lock the ring buffer.
        if !self.mutex.try_lock() {
            let cause: String = format!("could not lock ring buffer to push byte");
            warn!("try_push(): {}", &cause);
            return Ok(false);
        }
        // Write to the ring buffer.
        let result: Result<(), u16> = self.push_buf.try_enqueue(x);
        // Unlock the ring buffer.
        assert_eq!(self.mutex.unlock().is_ok(), true);
        // Return result.
        Ok(result.is_ok())
    }

    /// Closes the target ring.
    pub fn close(&mut self) -> Result<(), Fail> {
        // Attempt to push EoF.
        // Maximum number of retries. This is set to an arbitrary small value.
        for _ in 0..MAX_RETRIES_PUSH_EOF {
            match self.try_close() {
                Ok(()) => return Ok(()),
                Err(_) => continue,
            }
        }
        let cause: String = format!("failed to push EoF");
        error!("push_eof(): {}", cause);
        Err(Fail::new(libc::EIO, &cause))
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

        let result: Result<(), u16> = self.push_buf.try_enqueue(EOF);
        assert_eq!(self.mutex.unlock().is_ok(), true);

        match result {
            Ok(()) => Ok(()),
            Err(_) => Err(Fail::new(libc::EAGAIN, "Could not push EOF")),
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

    /// Aborts prepared state.
    pub fn abort(&mut self) {
        self.state_machine.abort();
    }
}
