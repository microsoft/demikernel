// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod pipe;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    futures::{
        pop::PopFuture,
        push::PushFuture,
        Operation,
        OperationResult,
    },
    pipe::Pipe,
};
use crate::{
    collections::shared_ring::SharedRingBuffer,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
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
    },
    QType,
};
use ::std::{
    any::Any,
    collections::HashMap,
    mem,
    rc::Rc,
};

//======================================================================================================================
// Constants
//======================================================================================================================

const RING_BUFFER_CAPACITY: usize = 4096;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes a memory queue.
pub struct CatmemLibOS {
    qtable: IoQueueTable,
    scheduler: Scheduler,
    rings: HashMap<QDesc, Pipe>,
}

impl MemoryRuntime for CatmemLibOS {}
//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for Catmem LibOS.
impl CatmemLibOS {
    /// Instantiates a new LibOS.
    pub fn new() -> Self {
        CatmemLibOS {
            qtable: IoQueueTable::new(),
            scheduler: Scheduler::default(),
            rings: HashMap::new(),
        }
    }

    /// Creates a new memory queue.
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("create_pipe() name={:?}", name);

        let ring: SharedRingBuffer<u16> = SharedRingBuffer::<u16>::create(name, RING_BUFFER_CAPACITY)?;

        let qd: QDesc = self.qtable.alloc(QType::MemoryQueue.into());
        assert_eq!(self.rings.insert(qd, Pipe::new(ring)).is_none(), true);

        Ok(qd)
    }

    /// Opens a memory queue.
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("open_pipe() name={:?}", name);

        let ring: SharedRingBuffer<u16> = SharedRingBuffer::<u16>::open(name, RING_BUFFER_CAPACITY)?;

        let qd: QDesc = self.qtable.alloc(QType::MemoryQueue.into());
        assert_eq!(self.rings.insert(qd, Pipe::new(ring)).is_none(), true);

        Ok(qd)
    }

    // Pushes EoF.
    fn push_eof(&mut self, ring: Rc<SharedRingBuffer<u16>>) -> Result<(), Fail> {
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
        match self.rings.remove(&qd) {
            Some(pipe) => {
                self.push_eof(pipe.buffer())?;
                self.qtable.free(qd);
                Ok(())
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        match self.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue push operation.
                match self.rings.get(&qd) {
                    Some(pipe) => {
                        let future: Operation = Operation::from(PushFuture::new(qd, pipe.buffer(), buf));
                        let handle: SchedulerHandle = match self.scheduler.insert(future) {
                            Some(handle) => handle,
                            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                        };
                        let qt: QToken = handle.into_raw().into();
                        trace!("push() qt={:?}", qt);
                        Ok(qt)
                    },
                    _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        // Issue pop operation.
        match self.rings.get(&qd) {
            Some(pipe) => {
                let future: Operation = Operation::from(PopFuture::new(qd, pipe.buffer()));
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
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
        let boxed_future: Box<dyn Any> = self.scheduler.take(handle).as_any();
        let boxed_concrete_type: Operation = *boxed_future.downcast::<Operation>().expect("Wrong type!");

        boxed_concrete_type.get_result()
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
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Pop(bytes, eof) => {
                // Handle end of file.
                if eof {
                    let pipe: &mut Pipe = self.rings.get_mut(&qd).expect("unregisted queue descriptor");
                    pipe.set_eof();
                }

                match self.into_sgarray(bytes) {
                    Ok(sga) => {
                        let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                        demi_qresult_t {
                            qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                            qr_qd: qd.into(),
                            qr_qt: qt.into(),
                            qr_value,
                        }
                    },
                    Err(e) => {
                        warn!("Operation Failed: {:?}", e);
                        demi_qresult_t {
                            qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                            qr_qd: qd.into(),
                            qr_qt: qt.into(),
                            qr_value: unsafe { mem::zeroed() },
                        }
                    },
                }
            },
            OperationResult::Failed(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt.into(),
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
