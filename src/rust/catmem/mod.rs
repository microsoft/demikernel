// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod queue;
mod ring;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::queue::SharedCatmemQueue;
use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        limits,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        queue::downcast_queue,
        types::demi_sgarray_t,
        Operation,
        OperationResult,
        SharedDemiRuntime,
        SharedObject,
    },
    QDesc,
    QToken,
};
use ::futures::FutureExt;
use ::std::{
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes bi-directional memory queues.
pub struct CatmemLibOS {
    runtime: SharedDemiRuntime,
}

#[derive(Clone)]
pub struct SharedCatmemLibOS(SharedObject<CatmemLibOS>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for Catmem LibOS.
impl CatmemLibOS {
    pub fn new(runtime: SharedDemiRuntime) -> Self {
        Self { runtime }
    }
}

/// Associate Functions for the shared Catmem LibOS
impl SharedCatmemLibOS {
    /// Instantiates a shared Catmem LibOS.
    pub fn new(_config: &Config, runtime: SharedDemiRuntime) -> Self {
        Self(SharedObject::new(CatmemLibOS::new(runtime)))
    }

    /// Creates a new memory queue.
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("create_pipe() name={:?}", name);
        let qd: QDesc = self
            .runtime
            .alloc_queue::<SharedCatmemQueue>(SharedCatmemQueue::create(name)?);

        Ok(qd)
    }

    /// Opens a memory queue.
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("open_pipe() name={:?}", name);

        let qd: QDesc = self
            .runtime
            .alloc_queue::<SharedCatmemQueue>(SharedCatmemQueue::open(name)?);

        Ok(qd)
    }

    /// Shutdown a consumer/pop-only queue. Currently, this is basically a no-op but it does cancel pending operations
    /// and free the queue from the IoQueueTable.
    pub fn shutdown(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("shutdown() qd={:?}", qd);
        let mut queue: SharedCatmemQueue = self.runtime.free_queue::<SharedCatmemQueue>(&qd)?;
        queue.shutdown()
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let mut queue: SharedCatmemQueue = self.runtime.free_queue::<SharedCatmemQueue>(&qd)?;
        queue.close()
    }

    /// Asynchronously close a socket.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("async_close() qd={:?}", qd);
        let mut queue: SharedCatmemQueue = self.get_queue(&qd)?;
        let coroutine_constructor = || -> Result<QToken, Fail> {
            let task_name: String = format!("Catmem::async_close for qd={:?}", qd);
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().close_coroutine(qd).fuse());
            self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
        };

        queue.async_close(coroutine_constructor)
    }

    pub async fn close_coroutine(mut self, qd: QDesc) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatmemQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for close operation to complete.
        match queue.do_async_close().await {
            // Operation completed successfully, thus free resources.
            Ok(()) => {
                // Release the queue descriptor, even if pushing EoF failed. This will prevent any further
                // operations on the queue, as well as it will ensure that the underlying shared ring buffer will
                // be eventually released.
                // Expect is safe here because we looked up the queue to schedule this coroutine and no other close
                // coroutine should be able to run due to state machine checks.
                self.runtime
                    .free_queue::<SharedCatmemQueue>(&qd)
                    .expect("queue should exist");
                (qd, OperationResult::Close)
            },
            // Operation failed, thus warn and return an error.
            Err(e) => {
                warn!("async_close(): {:?}", &e);
                (qd, OperationResult::Failed(e))
            },
        }
    }

    /// Pushes a scatter-gather array to a Push ring. If not a Push ring, then fail.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer (qd={:?})", qd);
            error!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        let task_name: String = format!("Catmem::push for qd={:?}", qd);
        let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().push_coroutine(qd, buf).fuse());

        self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
    }

    pub async fn push_coroutine(self, qd: QDesc, buf: DemiBuffer) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatmemQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Handle result.
        match queue.do_push(buf).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    /// Pops data from a Pop ring. If not a Pop ring, then return an error.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let task_name: String = format!("Catmem::pop for qd={:?}", qd);
        let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pop_coroutine(qd, size).fuse());

        self.runtime.clone().insert_io_coroutine(&task_name, coroutine)
    }

    pub async fn pop_coroutine(self, qd: QDesc, size: Option<usize>) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatmemQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for pop to complete.
        let (buf, _) = match queue.do_pop(size).await {
            Ok(result) => result,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        (qd, OperationResult::Pop(None, buf))
    }

    pub fn get_queue(&self, qd: &QDesc) -> Result<SharedCatmemQueue, Fail> {
        Ok(self.runtime.get_qtable().get::<SharedCatmemQueue>(qd)?.clone())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedCatmemLibOS {
    type Target = CatmemLibOS;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedCatmemLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl Drop for CatmemLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for boxed_queue in self.runtime.get_mut_qtable().drain() {
            if let Ok(mut catmem_queue) = downcast_queue::<SharedCatmemQueue>(boxed_queue) {
                if let Err(e) = catmem_queue.close() {
                    error!("push_eof() failed: {:?}", e);
                    warn!("leaking shared memory region");
                }
            }
        }
    }
}
