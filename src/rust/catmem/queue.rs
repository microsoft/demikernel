// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    ring::Ring,
    QMode,
};
use crate::{
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        queue::IoQueue,
        QType,
    },
    scheduler::{
        TaskHandle,
        Yielder,
        YielderHandle,
    },
};
use ::std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Maximum number of retries for pushing a EoF signal.
const MAX_RETRIES_PUSH_EOF: u32 = 16;

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure contains code for manipulating a single, Catmem-specific Demikernel queue. Queue state is kept in
/// the [ring] structure, while [pending_ops] holds the map of TaskHandles and YielderHandle for currently active async
/// functions.
#[derive(Clone)]
pub struct CatmemQueue {
    ring: Rc<RefCell<Ring>>,
    pending_ops: Rc<RefCell<HashMap<TaskHandle, YielderHandle>>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatmemQueue {
    /// This function creates a new CatmemQueue and a new shared ring buffer and connects to it to either the consumer
    /// or producer end indicated by [mode].
    pub fn create(name: &str, mode: QMode) -> Result<Self, Fail> {
        let pending_ops: Rc<RefCell<HashMap<TaskHandle, YielderHandle>>> =
            Rc::new(RefCell::<HashMap<TaskHandle, YielderHandle>>::new(HashMap::<
                TaskHandle,
                YielderHandle,
            >::new()));
        match mode {
            QMode::Push => Ok(Self {
                ring: Rc::new(RefCell::<Ring>::new(Ring::create_push_ring(name)?)),
                pending_ops,
            }),
            QMode::Pop => Ok(Self {
                ring: Rc::new(RefCell::<Ring>::new(Ring::create_pop_ring(name)?)),
                pending_ops,
            }),
        }
    }

    /// This function creates a new CatmemQueue and attaches to an existing share ring buffer as either a consumer or
    /// producer as indicated by [mode].
    pub fn open(name: &str, mode: QMode) -> Result<Self, Fail> {
        let pending_ops: Rc<RefCell<HashMap<TaskHandle, YielderHandle>>> =
            Rc::new(RefCell::<HashMap<TaskHandle, YielderHandle>>::new(HashMap::<
                TaskHandle,
                YielderHandle,
            >::new()));
        match mode {
            QMode::Push => Ok(Self {
                ring: Rc::new(RefCell::<Ring>::new(Ring::open_push_ring(name)?)),
                pending_ops,
            }),
            QMode::Pop => Ok(Self {
                ring: Rc::new(RefCell::<Ring>::new(Ring::open_pop_ring(name)?)),
                pending_ops,
            }),
        }
    }

    /// This function closes a ring endpoint.
    /// TODO merge this with async_close().
    pub fn close(&self) -> Result<(), Fail> {
        match &mut *self.ring.borrow_mut() {
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
            // Nothing to do on close for a pop ring.
            Ring::PopOnly(_) => Ok(()),
        }
    }

    /// Prepares a transition to the closing state.
    pub fn prepare_close(&self) -> Result<(), Fail> {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(ring) => ring.prepare_close(),
            Ring::PopOnly(ring) => ring.prepare_close(),
        }
    }

    /// Prepares a transition to the closed state.
    pub fn prepare_closed(&self) -> Result<(), Fail> {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(ring) => ring.prepare_closed(),
            Ring::PopOnly(ring) => ring.prepare_closed(),
        }
    }

    /// This function commits the queue to closing.
    pub fn commit(&self) {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(ring) => ring.commit(),
            Ring::PopOnly(ring) => ring.commit(),
        }
    }

    /// This function aborts a pending operation.
    pub fn abort(&self) {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(ring) => ring.abort(),
            Ring::PopOnly(_) => warn!("abort() called on a pop-only queue"),
        }
    }

    fn try_close(&self) -> Result<(), Fail> {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(ring) => ring.try_close(),
            Ring::PopOnly(_) => Ok(()),
        }
    }

    /// This function perms an async close on the target queue.
    pub async fn async_close(&self, yielder: Yielder) -> Result<(), Fail> {
        for _ in 0..MAX_RETRIES_PUSH_EOF {
            if let Ok(_) = self.try_close() {
                return Ok(());
            }
            if let Err(cause) = yielder.yield_once().await {
                return Err(cause);
            }
        }
        // We ran out of retries, thus fail.
        let cause: String = format!("failed to push EoF");
        error!("push_eof(): {}", cause);
        Err(Fail::new(libc::EIO, &cause))
    }

    /// This private function tries to pop from the queue and is mostly used for scoping the borrow.
    fn try_pop(&self) -> Result<(Option<u8>, bool), Fail> {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(_) => {
                let cause: &String = &format!("Cannot pop from push-only queue");
                error!("{}", &cause);
                Err(Fail::new(libc::EINVAL, cause))
            },
            Ring::PopOnly(ring) => Ok(ring.try_pop()?),
        }
    }

    /// This function pops a buffer of optional [size] from the queue. If the queue is connected to the push end of a
    /// shared memory ring, this function returns an error.
    pub async fn pop(&self, size: Option<usize>, yielder: Yielder) -> Result<(DemiBuffer, bool), Fail> {
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        let mut index: usize = 0;
        let eof: bool = loop {
            match self.try_pop()? {
                (Some(byte), eof) => {
                    if eof {
                        // If eof, then trim everything that we have received so far and return.
                        buf.trim(size - index)
                            .expect("cannot trim more bytes than the buffer has");
                        break true;
                    } else {
                        // If not eof, add byte to buffer.
                        buf[index] = byte;
                        index += 1;

                        // Check if we read enough bytes.
                        if index >= size {
                            // If so, trim buffer to length.
                            buf.trim(size - index)
                                .expect("cannot trim more bytes than the buffer has");
                            break false;
                        }
                    }
                },
                (None, _) => {
                    if index > 0 {
                        buf.trim(size - index)
                            .expect("cannot trim more bytes than the buffer has");
                        break false;
                    } else {
                        // Operation in progress. Check if cancelled.
                        match yielder.yield_once().await {
                            Ok(()) => continue,
                            Err(cause) => return Err(cause),
                        }
                    }
                },
            }
        };
        trace!("data read ({:?}/{:?} bytes, eof={:?})", buf.len(), size, eof);
        Ok((buf, eof))
    }

    /// This private function tries to push a single byte and is used for scoping the borrow.
    fn try_push(&self, byte: &u8) -> Result<bool, Fail> {
        match &mut *self.ring.borrow_mut() {
            Ring::PushOnly(ring) => Ok(ring.try_push(byte)?),
            Ring::PopOnly(_) => {
                let cause: &String = &format!("Cannot push to a pop-only queue");
                error!("{}", &cause);
                Err(Fail::new(libc::EINVAL, cause))
            },
        }
    }

    /// This function tries to push [buf] to the shared memory ring. If the queue is connected to the pop end, then
    /// this function returns an error.
    pub async fn push(&self, buf: DemiBuffer, yielder: Yielder) -> Result<(), Fail> {
        for byte in &buf[..] {
            loop {
                match self.try_push(byte)? {
                    true => break,
                    false => {
                        // Operation not completed. Check if it was cancelled.
                        match yielder.yield_once().await {
                            Ok(()) => continue,
                            Err(cause) => return Err(cause),
                        }
                    },
                }
            }
        }
        trace!("data written ({:?}/{:?} bytes)", buf.len(), buf.len());
        Ok(())
    }

    /// Adds a new operation to the list of pending operations on this queue.
    pub fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops
            .borrow_mut()
            .insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue.
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops
            .borrow_mut()
            .remove_entry(handle)
            .expect("operation should be registered");
    }

    /// Cancels all pending operations on this queue.
    pub fn cancel_pending_ops(&mut self, cause: Fail) {
        for (handle, mut yielder_handle) in self.pending_ops.borrow_mut().drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
        }
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatmemQueue {
    fn get_qtype(&self) -> QType {
        QType::MemoryQueue
    }
}
