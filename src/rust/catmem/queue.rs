// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::ring::{
        Ring,
        MAX_RETRIES_PUSH_EOF,
    },
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
        queue::IoQueue,
        scheduler::{
            TaskHandle,
            Yielder,
            YielderHandle,
        },
        DemiRuntime,
        QToken,
        QType,
        SharedObject,
    },
};
use ::std::{
    any::Any,
    collections::HashMap,
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure contains code for manipulating a single, Catmem-specific Demikernel queue. Queue state is kept in
/// the [ring] structure, while [pending_ops] holds the map of TaskHandles and YielderHandle for currently active async
/// functions.
pub struct CatmemQueue {
    ring: Ring,
    pending_ops: HashMap<TaskHandle, YielderHandle>,
}

#[derive(Clone)]

pub struct SharedCatmemQueue(SharedObject<CatmemQueue>);
//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatmemQueue {
    /// Creates a new [CatmemQueue] and a new shared ring buffer.
    pub fn create(name: &str) -> Result<Self, Fail> {
        Ok(Self {
            ring: Ring::create(name)?,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        })
    }

    /// Creates a new [CatmemQueue] and attaches it to an existing share ring buffer.
    pub fn open(name: &str) -> Result<Self, Fail> {
        Ok(Self {
            ring: Ring::open(name)?,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        })
    }
}

impl SharedCatmemQueue {
    pub fn create(name: &str) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(CatmemQueue::create(name)?)))
    }

    pub fn open(name: &str) -> Result<Self, Fail> {
        Ok(Self(SharedObject::new(CatmemQueue::open(name)?)))
    }

    pub fn shutdown(&mut self) -> Result<(), Fail> {
        {
            self.ring.prepare_close()?;
            self.ring.commit();
            self.ring.prepare_closed()?;
            self.ring.commit();
        }
        self.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was shutdown"));

        Ok(())
    }

    /// This function closes a ring endpoint.
    /// TODO merge this with async_close().
    pub fn close(&mut self) -> Result<(), Fail> {
        {
            self.ring.prepare_close()?;
            match self.ring.close() {
                Ok(()) => {
                    self.ring.commit();
                },
                Err(e) => {
                    self.ring.abort();
                    return Err(e);
                },
            }
        }
        self.ring.prepare_closed()?;
        self.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was closed"));
        self.ring.commit();
        Ok(())
    }

    /// Start an asynchronous coroutine to close this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run a close and any single-queue functionality after the close completes.
    pub fn async_close<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.ring.prepare_close()?;
        self.do_generic_sync_control_path_call(coroutine_constructor, false)
    }

    /// This function perms an async close on the target queue.
    pub async fn do_async_close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        let mut retries: u32 = MAX_RETRIES_PUSH_EOF;
        let x = loop {
            if let Ok(()) = self.ring.try_close() {
                break Ok(());
            }
            if let Err(cause) = yielder.yield_once().await {
                break Err(cause);
            }
            if retries == 0 {
                let cause: String = format!("failed to push EoF");
                error!("push_eof(): {}", cause);
                break Err(Fail::new(libc::EIO, &cause));
            }

            retries -= 1;
        };
        if x.is_err() {
            self.ring.abort();
            return x;
        }

        self.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was closed"));
        self.ring.commit();

        Ok(())
    }

    /// Schedule a coroutine to pop from this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to pop a buffer and any single-queue functionality after the pop completes.
    pub fn pop<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.do_generic_sync_data_path_call(coroutine_constructor)
    }

    /// This function pops a buffer of optional [size] from the queue. If the queue is connected to the push end of a
    /// shared memory ring, this function returns an error.
    pub async fn do_pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<(DemiBuffer, bool), Fail> {
        let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
        let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
        let eof: bool = loop {
            match self.ring.try_pop(&mut buf) {
                Ok((len, eof)) => {
                    if eof {
                        self.ring.prepare_close()?;
                        self.ring.commit();
                        buf.trim(size).expect("should be able to trim to a zero-length buffer");
                    } else {
                        buf.trim(size - len)
                            .expect("should be able to trim down to only read bytes");
                    }
                    break eof;
                },
                Err(e) if DemiRuntime::should_retry(e.errno) => {
                    // Operation in progress. Check if cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                },
                Err(e) => return Err(e),
            }
        };

        trace!("data read ({:?}/{:?} bytes, eof={:?})", buf.len(), size, eof);
        Ok((buf, eof))
    }

    /// Schedule a coroutine to push to this queue. This function contains all of the single-queue,
    /// asynchronous code necessary to run push a buffer and any single-queue functionality after the push completes.
    pub fn push<F>(&mut self, coroutine_constructor: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        self.do_generic_sync_data_path_call(coroutine_constructor)
    }

    /// This function tries to push [buf] to the shared memory ring. If the queue is connected to the pop end, then
    /// this function returns an error.
    pub async fn do_push(&mut self, mut buf: DemiBuffer, yielder: Yielder) -> Result<(), Fail> {
        loop {
            match self.ring.try_push(&buf) {
                Ok(len) if len == buf.len() => {
                    trace!("data written ({:?}/{:?} bytes)", buf.len(), buf.len());
                    return Ok(());
                },
                Ok(len) if len < buf.len() => {
                    buf.adjust(len).expect("should be able to split remaining bytes");
                    continue;
                },
                Ok(len) => unreachable!(
                    "should not be possible to write more than in the buffer (len={:?})",
                    len
                ),
                Err(e) if DemiRuntime::should_retry(e.errno) => {
                    // Operation not completed. Check if it was cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                },
                Err(e) => return Err(e),
            }
        }
    }

    /// Generic function for spawning a control-path coroutine on [self].
    fn do_generic_sync_control_path_call<F>(&mut self, coroutine: F, add_as_pending_op: bool) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        // Spawn coroutine.
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let task_handle: TaskHandle = match coroutine(yielder) {
            // We successfully spawned the coroutine.
            Ok(handle) => {
                // Commit the operation on the socket.
                self.ring.commit();
                handle
            },
            // We failed to spawn the coroutine.
            Err(e) => {
                // Abort the operation on the socket.
                self.ring.abort();
                return Err(e);
            },
        };

        // If requested, add this operation to the list of pending operations on this queue.
        if add_as_pending_op {
            self.add_pending_op(&task_handle, &yielder_handle);
        }

        Ok(task_handle.get_task_id().into())
    }

    /// Generic function for spawning a data-path coroutine on [self].
    fn do_generic_sync_data_path_call<F>(&mut self, coroutine: F) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Result<TaskHandle, Fail>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let task_handle: TaskHandle = coroutine(yielder)?;
        self.add_pending_op(&task_handle, &yielder_handle);
        Ok(task_handle.get_task_id().into())
    }

    /// Adds a new operation to the list of pending operations on this queue.
    pub fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops.insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue.
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops
            .remove_entry(handle)
            .expect("operation should be registered");
    }

    /// Cancels all pending operations on this queue.
    pub fn cancel_pending_ops(&mut self, cause: Fail) {
        for (handle, mut yielder_handle) in self.pending_ops.drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
        }
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl Deref for SharedCatmemQueue {
    type Target = CatmemQueue;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatmemQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
impl IoQueue for SharedCatmemQueue {
    fn get_qtype(&self) -> QType {
        QType::MemoryQueue
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}
