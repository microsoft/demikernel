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
        SchedulerHandle,
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

const RING_BUFFER_CAPACITY: usize = 4096;

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

    // Pushes EoF.
    fn push_eof(ring: Rc<SharedRingBuffer<u16>>) -> Result<(), Fail> {
        let x: u16 = ((1 & 0xff) << 8) as u16;

        loop {
            match ring.try_enqueue(x) {
                Ok(()) => break,
                Err(_) => {
                    warn!("failed to push EoF")
                },
            }
        }

        Ok(())
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        let mut qtable = self.qtable.borrow_mut();
        match qtable.get(&qd) {
            Some(queue) => Self::push_eof(queue.get_pipe().buffer())?,
            None => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };
        qtable.free(&qd);
        Ok(())
    }

    /// Pushes a scatter-gather array to a socket.
    /// TODO: Enforce semantics on the pipe.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        match self.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue push operation.
                match self.qtable.borrow().get(&qd) {
                    Some(queue) => {
                        let pipe: &Pipe = queue.get_pipe();
                        // Handle end of file.
                        if pipe.eof() {
                            let cause: String = format!("end of file (qd={:?})", qd);
                            error!("push(): {:?}", cause);
                            return Err(Fail::new(libc::ECONNRESET, &cause));
                        }
                        let future: PushFuture = PushFuture::new(pipe.buffer(), buf);
                        let coroutine: Pin<Box<Operation>> = Box::pin(async move {
                            // Wait for push to complete.
                            let result: Result<(), Fail> = future.await;
                            // Handle result.
                            match result {
                                Ok(()) => (qd, OperationResult::Push),
                                Err(e) => (qd, OperationResult::Failed(e)),
                            }
                        });
                        let task_id: String = format!("Catmem::push for qd={:?}", qd);
                        let task: OperationTask = OperationTask::new(task_id, coroutine);
                        let handle: SchedulerHandle = match self.scheduler.insert(task) {
                            Some(handle) => handle,
                            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                        };
                        let qt: QToken = handle.into_raw().into();
                        trace!("push() qt={:?}", qt);
                        Ok(qt)
                    },
                    None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    /// TODO: Enforce semantics on the pipe.
    pub fn pop(&mut self, qd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);
        let qtable = self.qtable.borrow();

        // Issue pop operation.
        match qtable.get(&qd) {
            Some(queue) => {
                let pipe: &Pipe = queue.get_pipe();
                // Handle end of file.
                if pipe.eof() {
                    let cause: String = format!("end of file (qd={:?})", qd);
                    error!("pop(): {:?}", cause);
                    return Err(Fail::new(libc::ECONNRESET, &cause));
                }

                let future: PopFuture = PopFuture::new(pipe.buffer(), size);
                let qtable_ptr: Rc<RefCell<IoQueueTable<CatmemQueue>>> = self.qtable.clone();
                let coroutine: Pin<Box<Operation>> = Box::pin(async move {
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
                });
                let task_id: String = format!("Catmem::pop for qd={:?}", qd);
                let task: OperationTask = OperationTask::new(task_id, coroutine);
                let handle: SchedulerHandle = match self.scheduler.insert(task) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                trace!("pop() qt={:?}", qt);
                Ok(qt)
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
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
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let task: OperationTask = OperationTask::from(self.scheduler.take(handle).as_any());
        task.get_result().expect("The coroutine has not finished")
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<SchedulerHandle, Fail> {
        match self.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: SchedulerHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
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
                        qr_ret: e.errno,
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
                    qr_ret: e.errno,
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
