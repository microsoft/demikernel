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
        types::{
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        Operation,
        OperationResult,
        OperationTask,
        QDesc,
        QToken,
        SharedDemiRuntime,
        SharedObject,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::std::{
    mem,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
};

#[cfg(feature = "profiler")]
use crate::timer;

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
        #[cfg(feature = "profiler")]
        timer!("catmem::new");
        Self(SharedObject::new(CatmemLibOS::new(runtime)))
    }

    /// Creates a new memory queue.
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::create_pipe");
        trace!("create_pipe() name={:?}", name);
        let qd: QDesc = self
            .runtime
            .alloc_queue::<SharedCatmemQueue>(SharedCatmemQueue::create(name)?);

        Ok(qd)
    }

    /// Opens a memory queue.
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::open_pipe");
        trace!("open_pipe() name={:?}", name);

        let qd: QDesc = self
            .runtime
            .alloc_queue::<SharedCatmemQueue>(SharedCatmemQueue::open(name)?);

        Ok(qd)
    }

    /// Shutdown a consumer/pop-only queue. Currently, this is basically a no-op but it does cancel pending operations
    /// and free the queue from the IoQueueTable.
    pub fn shutdown(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::shutdown");
        trace!("shutdown() qd={:?}", qd);
        let mut queue: SharedCatmemQueue = self.runtime.free_queue::<SharedCatmemQueue>(&qd)?;
        queue.shutdown()
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::close");
        trace!("close() qd={:?}", qd);
        let mut queue: SharedCatmemQueue = self.runtime.free_queue::<SharedCatmemQueue>(&qd)?;
        queue.close()
    }

    /// Asynchronously close a socket.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::async_close");
        trace!("async_close() qd={:?}", qd);
        let mut queue: SharedCatmemQueue = self.get_queue(&qd)?;
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().close_coroutine(qd, yielder));
            let task_name: String = format!("catmem::async_close for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        queue.async_close(coroutine)
    }

    pub async fn close_coroutine(mut self, qd: QDesc, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatmemQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for close operation to complete.
        match queue.do_async_close(yielder).await {
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
        #[cfg(feature = "profiler")]
        timer!("catmem::push");
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer (qd={:?})", qd);
            error!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        let mut queue: SharedCatmemQueue = self.get_queue(&qd)?;
        // Issue pop operation.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().push_coroutine(qd, buf, yielder));
            let task_name: String = format!("Catmem::push for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        queue.push(coroutine)
    }

    pub async fn push_coroutine(self, qd: QDesc, buf: DemiBuffer, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatmemQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        // Handle result.
        match queue.do_push(buf, yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    /// Pops data from a Pop ring. If not a Pop ring, then return an error.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::pop");
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let mut queue: SharedCatmemQueue = self.get_queue(&qd)?;
        // Issue pop operation.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(self.clone().pop_coroutine(qd, size, yielder));
            let task_name: String = format!("Catmem::pop for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        queue.pop(coroutine)
    }

    pub async fn pop_coroutine(self, qd: QDesc, size: Option<usize>, yielder: Yielder) -> (QDesc, OperationResult) {
        // Make sure the queue still exists.
        let mut queue: SharedCatmemQueue = match self.get_queue(&qd) {
            Ok(queue) => queue,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };

        // Wait for pop to complete.
        let (buf, _) = match queue.do_pop(size, yielder).await {
            Ok(result) => result,
            Err(e) => return (qd, OperationResult::Failed(e)),
        };
        (qd, OperationResult::Pop(None, buf))
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::alloc_sgarray");
        self.runtime.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::free_sgarray");
        self.runtime.free_sgarray(sga)
    }

    /// Takes out the [OperationResult] associated with the target [TaskHandle].
    fn take_result(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        #[cfg(feature = "profiler")]
        timer!("catmem::take_result");
        let task: OperationTask = self.runtime.remove_coroutine(&handle);
        let (qd, result): (QDesc, OperationResult) = task.get_result().expect("The coroutine has not finished");

        match self.get_queue(&qd) {
            Ok(mut queue) => queue.remove_pending_op(&handle),
            Err(_) => debug!("take_result(): this queue was closed (qd={:?})", qd),
        }

        (qd, result)
    }

    pub fn from_task_id(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::from_task_id");
        self.runtime.from_task_id(qt.into())
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.runtime.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.runtime.free_sgarray(sga)
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::pack_result");
        let (qd, result): (QDesc, OperationResult) = self.take_result(handle);
        let qr = match result {
            OperationResult::Push => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
                qr_qd: qd.into(),
                qr_qt: qt.into(),
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Pop(_, bytes) => match self.runtime.into_sgarray(bytes) {
                Ok(sga) => {
                    let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                    demi_qresult_t {
                        qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                        qr_qd: qd.into(),
                        qr_qt: qt.into(),
                        qr_ret: 0,
                        qr_value,
                    }
                },
                Err(e) => {
                    warn!("Operation Failed: {:?}", e);
                    demi_qresult_t {
                        qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                        qr_qd: qd.into(),
                        qr_qt: qt.into(),
                        qr_ret: e.errno as i64,
                        qr_value: unsafe { mem::zeroed() },
                    }
                },
            },
            OperationResult::Close => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_CLOSE,
                qr_qd: qd.into(),
                qr_qt: qt.into(),
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Failed(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt.into(),
                    qr_ret: e.errno as i64,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
            _ => panic!("This libOS does not support these operations"),
        };
        Ok(qr)
    }

    pub fn poll(&mut self) {
        #[cfg(feature = "profiler")]
        timer!("catmem::poll");
        self.runtime.poll()
    }

    pub fn get_queue(&self, qd: &QDesc) -> Result<SharedCatmemQueue, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::get_queue");
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
