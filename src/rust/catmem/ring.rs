// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::{
        concurrent_ring::ConcurrentRingBuffer,
        shared_ring::SharedRingBuffer,
    },
    runtime::{
        fail::Fail,
        network::ring::{
            operation::RingControlOperation,
            state::RingStateMachine,
        },
    },
};
use ::std::ptr::copy;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Header size for messages.
const HEADER_SIZE: usize = 4;

// Header for End of file (EoF) messages.
const EOF_MESSAGE_HEADER: [u8; HEADER_SIZE] = [0xD, 0xE, 0xA, 0xD];

/// Header for regular messages.
const REGULAR_MESSAGE_HEADER: [u8; HEADER_SIZE] = [0xB, 0xE, 0xE, 0xF];

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
    push_buf: SharedRingBuffer<ConcurrentRingBuffer>,
    /// Underlying buffer used for receiving data.
    pop_buf: SharedRingBuffer<ConcurrentRingBuffer>,
    /// Indicates whether the ring is open or closed.
    state_machine: RingStateMachine,
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
        })
    }

    /// Try to pop a byte from the shared memory ring. If successful, return the byte and whether the eof flag is set,
    /// otherwise return None for a retry.
    pub fn try_pop(&mut self, buf: &mut [u8]) -> Result<(usize, bool), Fail> {
        self.state_machine.may_pop()?;

        let mut msg: Vec<u8> = vec![0; buf.len() + HEADER_SIZE];
        // Read data from the ring buffer.
        let msg_len: usize = self.pop_buf.try_pop(&mut msg)? - HEADER_SIZE;

        // Check how many bytes were read.
        if msg_len > 0 {
            // We read some bytes. This should be a regular message,
            // thus copy it to the buffer.

            // Ensure that the message header is valid.
            debug_assert_eq!(REGULAR_MESSAGE_HEADER, msg[0..HEADER_SIZE]);

            // Copy the message to the buffer.
            unsafe {
                let buf_ptr: *mut u8 = buf.as_mut_ptr();
                let msg_ptr: *const u8 = msg.as_ptr();
                copy(msg_ptr.add(HEADER_SIZE), buf_ptr, msg_len);
            };

            Ok((msg_len, false))
        } else {
            // We read no bytes. This should be an EoF message.

            // Ensure that the message header is what we expect.
            debug_assert_eq!(EOF_MESSAGE_HEADER, msg[0..HEADER_SIZE]);

            Ok((0, true))
        }
    }

    /// Try to send a byte through the shared memory ring. If there is no space or another thread is writing to this
    /// ring, return [false], otherwise, return [true] if successfully enqueued.
    pub fn try_push(&mut self, buf: &[u8]) -> Result<usize, Fail> {
        self.state_machine.may_push()?;
        // Write the header.
        let mut msg: Vec<u8> = REGULAR_MESSAGE_HEADER.to_vec();
        msg.append(&mut buf.to_vec());

        // Write data to the ring buffer.
        Ok(self.push_buf.try_push(&msg)? - HEADER_SIZE)
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
        match self.push_buf.try_push(&EOF_MESSAGE_HEADER) {
            Ok(len) => {
                debug_assert_eq!(len, HEADER_SIZE);
                Ok(())
            },
            Err(_) => {
                let cause: String = format!("failed to push EoF");
                error!("try_close(): {:?}", &cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
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
