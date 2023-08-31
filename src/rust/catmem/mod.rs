// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod queue;
mod ring;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::queue::CatmemQueue;
use crate::{
    runtime::{
        fail::Fail,
        limits,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        queue::IoQueueTable,
        types::{
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
            demi_sgarray_t,
        },
        DemiRuntime,
        Operation,
        OperationResult,
        OperationTask,
        QDesc,
        QToken,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::std::{
    cell::RefCell,
    mem,
    pin::Pin,
    rc::Rc,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes a uni-directional memory queue.
/// TODO: Add support for bi-directional memory queues.
/// FIXME: https://github.com/microsoft/demikernel/issues/856
pub struct CatmemLibOS {
    qtable: Rc<RefCell<IoQueueTable<CatmemQueue>>>,
    runtime: DemiRuntime,
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl MemoryRuntime for CatmemLibOS {}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for Catmem LibOS.
impl CatmemLibOS {
    /// Instantiates a new LibOS.
    pub fn new(runtime: DemiRuntime) -> Self {
        #[cfg(feature = "profiler")]
        timer!("catmem::new");
        CatmemLibOS {
            qtable: Rc::new(RefCell::new(IoQueueTable::<CatmemQueue>::new())),
            runtime,
        }
    }

    /// Creates a new memory queue.
    pub fn create_pipe(&self, name: &str) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::create_pipe");
        trace!("create_pipe() name={:?}", name);
        let qd: QDesc = self.qtable.borrow_mut().alloc(CatmemQueue::create(name)?);

        Ok(qd)
    }

    /// Opens a memory queue.
    pub fn open_pipe(&self, name: &str) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::open_pipe");
        trace!("open_pipe() name={:?}", name);

        let qd: QDesc = self.qtable.borrow_mut().alloc(CatmemQueue::open(name)?);

        Ok(qd)
    }

    /// Shutdown a consumer/pop-only queue. Currently, this is basically a no-op but it does cancel pending operations
    /// and free the queue from the IoQueueTable.
    pub fn shutdown(&self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::shutdown");
        trace!("shutdown() qd={:?}", qd);
        self.get_queue(qd)?.shutdown()?;
        self.qtable.borrow_mut().free(&qd);
        Ok(())
    }

    /// Closes a memory queue.
    pub fn close(&self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::close");
        trace!("close() qd={:?}", qd);
        self.get_queue(qd)?.close()?;
        self.qtable.borrow_mut().free(&qd);
        Ok(())
    }

    /// Asynchronously close a socket.
    pub fn async_close(&self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::async_close");
        trace!("async_close() qd={:?}", qd);
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::close_coroutine(
                self.qtable.clone(),
                self.get_queue(qd)?.clone(),
                qd,
                yielder,
            ));
            let task_name: String = format!("catmem::async_close for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        Ok(self.get_queue(qd)?.async_close(coroutine)?)
    }

    pub async fn close_coroutine(
        qtable: Rc<RefCell<IoQueueTable<CatmemQueue>>>,
        mut queue: CatmemQueue,
        qd: QDesc,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Wait for close operation to complete.
        match queue.do_async_close(yielder).await {
            // Operation completed successfully, thus free resources.
            Ok(()) => {
                // Release the queue descriptor, even if pushing EoF failed. This will prevent any further
                // operations on the queue, as well as it will ensure that the underlying shared ring buffer will
                // be eventually released.
                qtable.borrow_mut().free(&qd);
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
    pub fn push(&self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::push");
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.clone_sgarray(sga)?;

        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer (qd={:?})", qd);
            error!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        let queue: CatmemQueue = self.get_queue(qd)?;
        // Issue pop operation.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::push_coroutine(qd, queue.clone(), buf, yielder));
            let task_name: String = format!("Catmem::push for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        queue.push(coroutine)
    }

    pub async fn push_coroutine(
        qd: QDesc,
        queue: CatmemQueue,
        buf: DemiBuffer,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
        // Handle result.
        match queue.do_push(buf, yielder).await {
            Ok(()) => (qd, OperationResult::Push),
            Err(e) => (qd, OperationResult::Failed(e)),
        }
    }

    /// Pops data from a Pop ring. If not a Pop ring, then return an error.
    pub fn pop(&self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::pop");
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        let queue: CatmemQueue = self.get_queue(qd)?;
        // Issue pop operation.
        let coroutine = |yielder: Yielder| -> Result<TaskHandle, Fail> {
            let coroutine: Pin<Box<Operation>> = Box::pin(Self::pop_coroutine(qd, queue.clone(), size, yielder));
            let task_name: String = format!("Catmem::pop for qd={:?}", qd);
            self.runtime.insert_coroutine(&task_name, coroutine)
        };
        queue.pop(coroutine)
    }

    pub async fn pop_coroutine(
        qd: QDesc,
        queue: CatmemQueue,
        size: Option<usize>,
        yielder: Yielder,
    ) -> (QDesc, OperationResult) {
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

        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => queue.remove_pending_op(&handle),
            None => debug!("take_result(): this queue was closed (qd={:?})", qd),
        }

        (qd, result)
    }

    pub fn from_task_id(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::from_task_id");
        self.runtime.from_task_id(qt.into())
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
            OperationResult::Pop(_, bytes) => match self.into_sgarray(bytes) {
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

    pub fn poll(&self) {
        #[cfg(feature = "profiler")]
        timer!("catmem::poll");
        self.runtime.poll()
    }

    pub fn get_queue(&self, qd: QDesc) -> Result<CatmemQueue, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catmem::get_queue");
        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => Ok(queue.clone()),
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    pub fn get_qtable(&self) -> Rc<RefCell<IoQueueTable<CatmemQueue>>> {
        #[cfg(feature = "profiler")]
        timer!("catmem::get_qtable");
        self.qtable.clone()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for CatmemLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for mut queue in self.qtable.borrow_mut().drain() {
            if let Err(e) = queue.close() {
                error!("push_eof() failed: {:?}", e);
                warn!("leaking shared memory region");
            }
        }
    }
}
