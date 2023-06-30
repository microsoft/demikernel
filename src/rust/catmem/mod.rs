// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod pipe;
mod queue;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    futures::OperationResult,
    pipe::Pipe,
    queue::CatmemQueue,
};
use crate::{
    catmem::{
        futures::{
            close::{
                close_coroutine,
                push_eof,
            },
            pop::pop_coroutine,
            push::push_coroutine,
        },
        pipe::PipeState,
    },
    collections::shared_ring::SharedRingBuffer,
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
// Constants
//======================================================================================================================

/// Capacity of the ring buffer, in bytes.
/// This does not correspond to the effective number of bytes that may be stored in the ring buffer due to layout and
/// padding. Still, this is intentionally set so as the effective capacity is large enough to hold 16 KB of data.
const RING_BUFFER_CAPACITY: usize = 65536;

//======================================================================================================================
// Types
//======================================================================================================================

// TODO: Remove this once we unify return types.
type Operation = dyn Future<Output = (QDesc, OperationResult)>;
type OperationTask = TaskWithResult<(QDesc, OperationResult)>;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes a memory queue.
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

    /// Creates a new memory queue.
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("create_pipe() name={:?}", name);

        let ring: SharedRingBuffer<u16> = SharedRingBuffer::<u16>::create(name, RING_BUFFER_CAPACITY)?;
        let qd: QDesc = self.qtable.borrow_mut().alloc(CatmemQueue::new(ring));

        Ok(qd)
    }

    /// Opens a memory queue.
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("open_pipe() name={:?}", name);

        let ring: SharedRingBuffer<u16> = SharedRingBuffer::<u16>::open(name, RING_BUFFER_CAPACITY)?;
        let qd: QDesc = self.qtable.borrow_mut().alloc(CatmemQueue::new(ring));

        Ok(qd)
    }

    /// Disallows further operations on a memory queue.
    /// This causes the queue descriptor to be freed and the underlying ring
    /// buffer to be released, but it does not push an EoF message to the other end.
    pub fn shutdown(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("shutdown() qd={:?}", qd);
        let mut qtable = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(queue) => {
                // Set pipe as closing.
                let pipe: &mut Pipe = queue.get_mut_pipe();
                pipe.set_state(PipeState::Closing);

                queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was closed"));

                // Set pipe as closed.
                let pipe: &mut Pipe = queue.get_mut_pipe();
                pipe.set_state(PipeState::Closed);

                qtable.free(&qd);
            },
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("shutdown(): {}", cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        };
        Ok(())
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatmemQueue>> = self.qtable.borrow_mut();

        // Check if queue descriptor is valid.
        match qtable.get_mut(&qd) {
            Some(queue) => {
                let pipe: &mut Pipe = queue.get_mut_pipe();

                // Set pipe as closing.
                pipe.set_state(PipeState::Closing);

                let ring: Rc<SharedRingBuffer<u16>> = pipe.buffer();

                // Attempt to push EoF.
                let result: Result<(), Fail> = { push_eof(ring) };
                queue.cancel_pending_ops(Fail::new(libc::ECANCELED, "this queue was closed"));

                // Release the queue descriptor, even if pushing EoF failed. This will prevent any further operations on the
                // queue, as well as it will ensure that the underlying shared ring buffer will be eventually released.
                qtable.free(&qd);
                result
            },
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("close(): {}", cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        }
    }

    /// Asynchronously close a socket.
    pub fn async_close(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("async_close() qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatmemQueue>> = self.qtable.borrow_mut();

        // Check if queue descriptor is valid.
        match qtable.get_mut(&qd) {
            Some(queue) => {
                let pipe: &mut Pipe = queue.get_mut_pipe();

                // Set pipe as closing.
                pipe.set_state(PipeState::Closing);

                let ring: Rc<SharedRingBuffer<u16>> = pipe.buffer();
                let qtable_ptr: Rc<RefCell<IoQueueTable<CatmemQueue>>> = self.qtable.clone();
                let yielder: Yielder = Yielder::new();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                    // Wait for close operation to complete.
                    let result: Result<(), Fail> = close_coroutine(ring, yielder).await;

                    // Handle result.
                    match result {
                        // Operation completed successfully, thus free resources.
                        Ok(()) => {
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

                            // Release the queue descriptor, even if pushing EoF failed. This will prevent any further operations on the
                            // queue, as well as it will ensure that the underlying shared ring buffer will be eventually released.
                            qtable_.free(&qd);
                            (qd, OperationResult::Close)
                        },
                        // Operation failed, thus warn and return an error.
                        Err(e) => {
                            warn!("async_close(): {:?}", &e);
                            (qd, OperationResult::Failed(e))
                        },
                    }
                });

                // Schedule coroutine.
                let task_name: String = format!("catmem::async_close for qd={:?}", qd);
                let task: OperationTask = OperationTask::new(task_name, coroutine);
                let handle: TaskHandle = match self.scheduler.insert(task) {
                    Some(handle) => handle,
                    None => {
                        let cause: String = format!("cannot schedule coroutine (qd={:?})", qd);
                        error!("async_close(): {}", &cause);
                        return Err(Fail::new(libc::EAGAIN, &cause));
                    },
                };
                Ok(handle.get_task_id().into())
            },
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("async_close(): {}", cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    /// Pushes a scatter-gather array to a socket.
    /// TODO: Enforce semantics on the pipe.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        match self.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    let cause: String = format!("zero-length buffer (qd={:?})", qd);
                    error!("push(): {}", cause);
                    return Err(Fail::new(libc::EINVAL, &cause));
                }

                // Issue push operation.
                match self.qtable.borrow_mut().get_mut(&qd) {
                    Some(queue) => {
                        let pipe: &mut Pipe = queue.get_mut_pipe();

                        // Check if the pipe is closing or closed.
                        if pipe.state() == PipeState::Closing {
                            let cause: String = format!("pipe is closing (qd={:?})", qd);
                            error!("push(): {}", cause);
                            return Err(Fail::new(libc::EBADF, &cause));
                        } else if pipe.state() == PipeState::Closed {
                            let cause: String = format!("pipe is closed (qd={:?})", qd);
                            error!("push(): {}", cause);
                            return Err(Fail::new(libc::EBADF, &cause));
                        }

                        // TODO: review the following code once that condition is enforced by the pipe abstraction.
                        // We do not check for EoF because pipes are unidirectional,
                        // and if EoF is set pipe for a push-only pipe, that pipe is closed.
                        if pipe.eof() {
                            unreachable!("push() called on a closed pipe");
                        }

                        // Create co-routine.
                        let ring: Rc<SharedRingBuffer<u16>> = pipe.buffer();
                        let yielder: Yielder = Yielder::new();
                        let yielder_handle: YielderHandle = yielder.get_handle();
                        let coroutine: Pin<Box<Operation>> = {
                            Box::pin(async move {
                                // Wait for push to complete.
                                let result: Result<(), Fail> = push_coroutine(ring, buf, yielder).await;
                                // Handle result.
                                match result {
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
                        queue.add_pending_op(&handle, &yielder_handle);
                        let qt: QToken = handle.get_task_id().into();
                        trace!("push() qt={:?}", qt);
                        Ok(qt)
                    },
                    None => {
                        let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                        error!("push(): {}", cause);
                        Err(Fail::new(libc::EBADF, &cause))
                    },
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    /// TODO: Enforce semantics on the pipe.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}, size={:?}", qd, size);

        // We just assert 'size' here, because it was previously checked at PDPIX layer.
        debug_assert!(size.is_none() || ((size.unwrap() > 0) && (size.unwrap() <= limits::POP_SIZE_MAX)));

        // Issue pop operation.
        match self.qtable.borrow_mut().get_mut(&qd) {
            Some(queue) => {
                let pipe: &mut Pipe = queue.get_mut_pipe();

                // Check if the pipe is closing or closed.
                if pipe.state() == PipeState::Closing {
                    let cause: String = format!("pipe is closing (qd={:?})", qd);
                    error!("push(): {}", cause);
                    return Err(Fail::new(libc::EBADF, &cause));
                } else if pipe.state() == PipeState::Closed {
                    let cause: String = format!("pipe is closed (qd={:?})", qd);
                    error!("push(): {}", cause);
                    return Err(Fail::new(libc::EBADF, &cause));
                }

                let ring: Rc<SharedRingBuffer<u16>> = pipe.buffer();
                let yielder: Yielder = Yielder::new();
                let yielder_handle: YielderHandle = yielder.get_handle();
                let coroutine: Pin<Box<Operation>> = if pipe.eof() {
                    // Handle end of file.
                    Box::pin(async move {
                        let cause: String = format!("connection reset (qd={:?})", qd);
                        error!("pop(): {:?}", &cause);
                        (qd, OperationResult::Failed(Fail::new(libc::ECONNRESET, &cause)))
                    })
                } else {
                    let qtable_ptr: Rc<RefCell<IoQueueTable<CatmemQueue>>> = self.qtable.clone();
                    Box::pin(async move {
                        // Wait for pop to complete.
                        let result: Result<(DemiBuffer, bool), Fail> = pop_coroutine(ring, size, yielder).await;
                        // Process the result.
                        match result {
                            Ok((buf, eof)) => {
                                if eof {
                                    let mut qtable_: RefMut<IoQueueTable<CatmemQueue>> = qtable_ptr.borrow_mut();
                                    let queue: &mut CatmemQueue = match qtable_.get_mut(&qd) {
                                        Some(queue) => queue,
                                        None => {
                                            let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                                            error!("pop(): {}", cause);
                                            return (qd, OperationResult::Failed(Fail::new(libc::EBADF, &cause)));
                                        },
                                    };
                                    let pipe: &mut Pipe = queue.get_mut_pipe();
                                    pipe.set_eof();
                                }
                                (qd, OperationResult::Pop(buf))
                            },
                            Err(e) => (qd, OperationResult::Failed(e)),
                        }
                    })
                };

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
                queue.add_pending_op(&handle, &yielder_handle);
                let qt: QToken = handle.get_task_id().into();
                trace!("pop() qt={:?}", qt);
                Ok(qt)
            },
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("pop(): {}", cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
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
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for CatmemLibOS {
    // Releases all sockets allocated by Catnap.
    fn drop(&mut self) {
        for (_, queue) in self.qtable.borrow().get_values() {
            if let Err(e) = push_eof(queue.get_pipe().buffer()) {
                error!("push_eof() failed: {:?}", e);
                warn!("leaking shared memory region");
            }
        }
    }
}
