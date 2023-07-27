// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod queue;
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
        QDesc,
        QToken,
    },
    scheduler::{
        Scheduler,
        TaskHandle,
        TaskWithResult,
        Yielder,
        YielderHandle,
    },
};
use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    future::Future,
    mem,
    pin::Pin,
    rc::Rc,
};

//======================================================================================================================
// Types
//======================================================================================================================

// TODO: Remove this once we unify return types.
type Operation = dyn Future<Output = (QDesc, OperationResult)>;
type OperationTask = TaskWithResult<(QDesc, OperationResult)>;

#[derive(Clone)]
/// Operation Result
pub enum OperationResult {
    Push,
    Pop(DemiBuffer),
    Close,
    Failed(Fail),
}

/// Whether this queue is open for push or pop (i.e., producer or consumer).
pub enum QMode {
    Push,
    Pop,
}

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes a uni-directional memory queue.
/// TODO: Add support for bi-directional memory queues.
/// FIXME: https://github.com/microsoft/demikernel/issues/856
pub struct CatmemLibOS {
    qtable: Rc<RefCell<IoQueueTable<CatmemQueue>>>,
    scheduler: Scheduler,
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
    pub fn new() -> Self {
        CatmemLibOS {
            qtable: Rc::new(RefCell::new(IoQueueTable::<CatmemQueue>::new())),
            scheduler: Scheduler::default(),
        }
    }

    /// Creates a new memory queue and connects to the [mode] end.
    pub fn create_pipe(&mut self, name: &str, mode: QMode) -> Result<QDesc, Fail> {
        trace!("create_pipe() name={:?}", name);
        let qd: QDesc = self.qtable.borrow_mut().alloc(CatmemQueue::create(name, mode)?);

        Ok(qd)
    }

    /// Opens a memory queue and connects to the [mode] end.
    pub fn open_pipe(&mut self, name: &str, mode: QMode) -> Result<QDesc, Fail> {
        trace!("open_pipe() name={:?}", name);

        let qd: QDesc = self.qtable.borrow_mut().alloc(CatmemQueue::open(name, mode)?);

        Ok(qd)
    }

    /// Shutdown a consumer/pop-only queue. Currently, this is basically a no-op but it does cancel pending operations
    /// and free the queue from the IoQueueTable.
    pub fn shutdown(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("shutdown() qd={:?}", qd);
        let mut queue: CatmemQueue = self.get_queue(qd)?;
        queue.prepare_close()?;
        queue.commit();
        queue.prepare_closed()?;
        queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was shutdown"));
        queue.commit();
        self.qtable.borrow_mut().free(&qd);
        Ok(())
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let mut queue: CatmemQueue = self.get_queue(qd)?;
        queue.prepare_close()?;
        match queue.close() {
            Ok(()) => {
                queue.commit();
                queue.prepare_closed()?;
                queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was closed"));
                queue.commit();
                self.qtable.borrow_mut().free(&qd);
                Ok(())
            },
            Err(e) => {
                queue.abort();
                Err(e)
            },
        }
    }

    /// Asynchronously close a socket.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("async_close() qd={:?}", qd);
        let queue: CatmemQueue = self.get_queue(qd)?;
        // Check that queue is not already closed and move to the closing state.
        queue.prepare_close()?;

        let qtable_ptr: Rc<RefCell<IoQueueTable<CatmemQueue>>> = self.qtable.clone();
        let yielder: Yielder = Yielder::new();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            if let Err(e) = queue.prepare_closed() {
                warn!("async_close() qd={:?}: {:?}", qd, &e);
                return (qd, OperationResult::Failed(e));
            }
            // Wait for close operation to complete.
            match queue.async_close(yielder).await {
                // Operation completed successfully, thus free resources.
                Ok(()) => {
                    queue.commit();
                    let mut qtable_: RefMut<IoQueueTable<CatmemQueue>> = qtable_ptr.borrow_mut();
                    match qtable_.get_mut(&qd) {
                        Some(queue) => {
                            // Cancel all pending operations.
                            queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was closed"));
                        },
                        None => {
                            let cause: &String = &format!("invalid queue descriptor: {:?}", qd);
                            error!("{}", &cause);
                            return (qd, OperationResult::Failed(Fail::new(libc::EBADF, cause)));
                        },
                    }

                    // Release the queue descriptor, even if pushing EoF failed. This will prevent any further
                    // operations on the queue, as well as it will ensure that the underlying shared ring buffer will
                    // be eventually released.
                    qtable_.free(&qd);
                    (qd, OperationResult::Close)
                },
                // Operation failed, thus warn and return an error.
                Err(e) => {
                    queue.abort();
                    warn!("async_close(): {:?}", &e);
                    (qd, OperationResult::Failed(e))
                },
            }
        });
        {
            // Schedule coroutine.
            let queue: CatmemQueue = self.get_queue(qd)?;
            let task_name: String = format!("catmem::async_close for qd={:?}", qd);
            let task: OperationTask = OperationTask::new(task_name, coroutine);
            let handle: TaskHandle = match self.scheduler.insert(task) {
                Some(handle) => {
                    queue.commit();
                    handle
                },
                None => {
                    queue.abort();
                    let cause: String = format!("cannot schedule coroutine (qd={:?})", qd);
                    error!("async_close(): {}", &cause);
                    return Err(Fail::new(libc::EAGAIN, &cause));
                },
            };
            Ok(handle.get_task_id().into())
        }
    }

    /// Pushes a scatter-gather array to a Push ring. If not a Push ring, then fail.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let buf: DemiBuffer = self.clone_sgarray(sga)?;

        if buf.len() == 0 {
            let cause: String = format!("zero-length buffer (qd={:?})", qd);
            error!("push(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }
        let queue: CatmemQueue = self.get_queue(qd)?;

        // Create co-routine.
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let coroutine: Pin<Box<Operation>> = {
            Box::pin(async move {
                // Handle result.
                match queue.push(buf, yielder).await {
                    Ok(()) => (qd, OperationResult::Push),
                    Err(e) => (qd, OperationResult::Failed(e)),
                }
            })
        };
        let task_id: String = format!("Catmem::push for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        let handle: TaskHandle = match self.scheduler.insert(task) {
            Some(handle) => handle,
            None => {
                let cause: String = format!("cannot schedule co-routine (qd={:?})", qd);
                error!("push(): {}", cause);
                return Err(Fail::new(libc::EAGAIN, &cause));
            },
        };
        self.get_queue(qd)?.add_pending_op(&handle, &yielder_handle);
        let qt: QToken = handle.get_task_id().into();
        trace!("push() qt={:?}", qt);
        Ok(qt)
    }

    /// Pops data from a Pop ring. If not a Pop ring, then return an error.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        // Issue pop operation.
        let queue: CatmemQueue = self.get_queue(qd)?;
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let qtable_ptr: Rc<RefCell<IoQueueTable<CatmemQueue>>> = self.qtable.clone();
        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
            // Wait for pop to complete.
            let (buf, eof) = match queue.pop(size, yielder).await {
                Ok((buf, eof)) => (buf, eof),
                Err(e) => return (qd, OperationResult::Failed(e)),
            };

            let mut qtable_: RefMut<IoQueueTable<CatmemQueue>> = qtable_ptr.borrow_mut();

            if eof {
                match qtable_.get_mut(&qd) {
                    Some(queue) => {
                        if let Err(e) = queue.prepare_close() {
                            warn!("pop() qd={:?}: {:?}", qd, &e);
                            return (qd, OperationResult::Failed(e));
                        }
                        queue.commit();
                    },
                    None => {
                        let cause: &String = &format!("invalid queue descriptor: {:?}", qd);
                        error!("{}", &cause);
                        return (qd, OperationResult::Failed(Fail::new(libc::EBADF, cause)));
                    },
                };
            };

            (qd, OperationResult::Pop(buf))
        });

        let task_id: String = format!("Catmem::pop for qd={:?}", qd);
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        let handle: TaskHandle = match self.scheduler.insert(task) {
            Some(handle) => handle,
            None => {
                let cause: String = format!("cannot schedule co-routine (qd={:?})", qd);
                error!("pop(): {}", cause);
                return Err(Fail::new(libc::EAGAIN, &cause));
            },
        };
        self.get_queue(qd)?.add_pending_op(&handle, &yielder_handle);
        let qt: QToken = handle.get_task_id().into();
        trace!("pop() qt={:?}", qt);
        Ok(qt)
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        MemoryRuntime::alloc_sgarray(self, size)
    }

    /// Releases a scatter-gather array.
    pub fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        MemoryRuntime::free_sgarray(self, sga)
    }

    /// Takes out the [OperationResult] associated with the target [TaskHandle].
    fn take_result(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        let task: OperationTask = if let Some(task) = self.scheduler.remove(&handle) {
            OperationTask::from(task.as_any())
        } else {
            panic!("Removing task that does not exist (either was previously removed or never inserted)");
        };
        let (qd, result): (QDesc, OperationResult) = task.get_result().expect("The coroutine has not finished");

        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => queue.remove_pending_op(&handle),
            None => debug!("take_result(): this queue was closed (qd={:?})", qd),
        }

        (qd, result)
    }

    pub fn from_task_id(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => {
                let cause: String = format!("invalid queue token (qt={:?})", qt);
                error!("from_task_id(): {}", cause);
                Err(Fail::new(libc::EINVAL, &cause))
            },
        }
    }

    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        let (qd, result): (QDesc, OperationResult) = self.take_result(handle);
        let qr = match result {
            OperationResult::Push => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
                qr_qd: qd.into(),
                qr_qt: qt.into(),
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Pop(bytes) => match self.into_sgarray(bytes) {
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
        };
        Ok(qr)
    }

    pub fn poll(&self) {
        self.scheduler.poll()
    }

    fn get_queue(&self, qd: QDesc) -> Result<CatmemQueue, Fail> {
        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => Ok(queue.clone()),
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for CatmemLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for queue in self.qtable.borrow_mut().drain() {
            if let Err(e) = queue.close() {
                error!("push_eof() failed: {:?}", e);
                warn!("leaking shared memory region");
            }
        }
    }
}
