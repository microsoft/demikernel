// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod pipe;
mod queue;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    futures::{
        pop::PopFuture,
        push::PushFuture,
        OperationResult,
    },
    pipe::Pipe,
    queue::CatmemQueue,
};
use crate::{
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
    /// End-of-file (EoF) signal.
    const EOF: u16 = ((1 & 0xff) << 8);

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
        match qtable.get(&qd) {
            Some(_) => {
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

    /// Pushes the EoF signal to a shared ring buffer.
    fn push_eof(ring: Rc<SharedRingBuffer<u16>>) -> Result<(), Fail> {
        // Maximum number of retries. This is set to an arbitrary small value.
        let mut retries: u32 = 16;

        loop {
            match ring.try_enqueue(Self::EOF) {
                Ok(()) => break,
                Err(_) => {
                    retries -= 1;
                    if retries == 0 {
                        let cause: String = format!("failed to push EoF");
                        error!("push_eof(): {}", cause);
                        return Err(Fail::new(libc::EIO, &cause));
                    }
                },
            }
        }

        Ok(())
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let mut qtable: RefMut<IoQueueTable<CatmemQueue>> = self.qtable.borrow_mut();

        // Check if queue descriptor is valid.
        if qtable.get(&qd).is_none() {
            let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
            error!("close(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }

        // Attempt to push EoF.
        let result: Result<(), Fail> = {
            // It is safe to call expect() here because the queue descriptor is guaranteed to be valid.
            let queue: &CatmemQueue = qtable.get(&qd).expect("queue descriptor should be valid");
            Self::push_eof(queue.get_pipe().buffer())
        };

        // Release the queue descriptor, even if pushing EoF failed. This will prevent any further operations on the
        // queue, as well as it will ensure that the underlying shared ring buffer will be eventually released.
        qtable.free(&qd);

        result
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
                match self.qtable.borrow().get(&qd) {
                    Some(queue) => {
                        let pipe: &Pipe = queue.get_pipe();

                        // TODO: review the following code once that condition is enforced by the pipe abstraction.
                        // We do not check for EoF because pipes are unidirectional,
                        // and if EoF is set pipe for a push-only pipe, that pipe is closed.
                        if pipe.eof() {
                            unreachable!("push() called on a closed pipe");
                        }

                        let coroutine: Pin<Box<Operation>> = {
                            let future: PushFuture = PushFuture::new(pipe.buffer(), buf);
                            Box::pin(async move {
                                // Wait for push to complete.
                                let result: Result<(), Fail> = future.await;
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
        match self.qtable.borrow().get(&qd) {
            Some(queue) => {
                let pipe: &Pipe = queue.get_pipe();
                let coroutine: Pin<Box<Operation>> = if pipe.eof() {
                    // Handle end of file.
                    Box::pin(async move {
                        let cause: String = format!("connection reset (qd={:?})", qd);
                        error!("pop(): {:?}", &cause);
                        (qd, OperationResult::Failed(Fail::new(libc::ECONNRESET, &cause)))
                    })
                } else {
                    let future: PopFuture = PopFuture::new(pipe.buffer(), size);
                    let qtable_ptr: Rc<RefCell<IoQueueTable<CatmemQueue>>> = self.qtable.clone();
                    Box::pin(async move {
                        // Wait for pop to complete.
                        let result: Result<(DemiBuffer, bool), Fail> = future.await;
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

    /// Takes out the [OperationResult] associated with the target [SchedulerHandle].
    fn take_result(&mut self, handle: TaskHandle) -> (QDesc, OperationResult) {
        let task: OperationTask = OperationTask::from(self.scheduler.remove(handle).as_any());
        task.get_result().expect("The coroutine has not finished")
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => {
                let cause: String = format!("invalid queue token (qt={:?})", qt);
                error!("schedule(): {}", cause);
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
