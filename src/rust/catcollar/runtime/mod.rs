// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod network;

//==============================================================================
// Imports
//==============================================================================

use super::iouring::IoUring;
use crate::{
    pal::{
        data_structures::SockAddr,
        linux,
    },
    runtime::{
        fail::Fail,
        liburing,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        Operation,
        OperationTask,
        SharedObject,
    },
    scheduler::{
        scheduler::Scheduler,
        TaskHandle,
    },
};
use ::std::{
    collections::{
        HashMap,
        HashSet,
    },
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    os::unix::prelude::RawFd,
    pin::Pin,
};

//==============================================================================
// Constants
//==============================================================================

/// Number of slots in an I/O User ring.
const CATCOLLAR_NUM_RINGS: u32 = 128;

//==============================================================================
// Structures
//==============================================================================

/// Request ID
#[derive(Clone, Copy, Hash, Debug, Eq, PartialEq)]
pub struct RequestId(pub *const liburing::msghdr);

/// I/O User Ring Runtime
pub struct IoUringRuntime {
    /// Scheduler
    pub scheduler: Scheduler,
    /// Underlying io_uring.
    io_uring: IoUring,
    /// Pending requests.
    pending: HashSet<RequestId>,
    /// Completed requests.
    completed: HashMap<RequestId, (Option<SocketAddrV4>, i32)>,
}

#[derive(Clone)]
pub struct SharedIoUringRuntime(SharedObject<IoUringRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for I/O User Ring Runtime
impl SharedIoUringRuntime {
    /// Pushes a buffer to the target I/O user ring.
    pub fn push(&mut self, sockfd: RawFd, buf: DemiBuffer) -> Result<RequestId, Fail> {
        let msg_ptr: *const liburing::msghdr = self.io_uring.push(sockfd, buf)?;
        let request_id: RequestId = RequestId(msg_ptr);
        self.pending.insert(request_id);
        Ok(request_id)
    }

    /// Pushes a buffer to the target I/O user ring.
    pub fn pushto(&mut self, sockfd: i32, addr: SocketAddrV4, buf: DemiBuffer) -> Result<RequestId, Fail> {
        let msg_ptr: *const liburing::msghdr = self.io_uring.pushto(sockfd, addr, buf)?;
        let request_id: RequestId = RequestId(msg_ptr);
        self.pending.insert(request_id);
        Ok(request_id)
    }

    /// Pops a buffer from the target I/O user ring.
    pub fn pop(&mut self, sockfd: RawFd, buf: DemiBuffer) -> Result<RequestId, Fail> {
        let msg_ptr: *const liburing::msghdr = self.io_uring.pop(sockfd, buf)?;
        let request_id: RequestId = RequestId(msg_ptr);
        self.pending.insert(request_id);
        Ok(request_id)
    }

    /// Peeks for the completion of an operation in the target I/O user ring.
    pub fn peek(&mut self, request_id: RequestId) -> Result<(Option<SocketAddrV4>, i32), Fail> {
        // Check if pending request has completed.
        match self.completed.remove(&request_id) {
            // The target request has already completed.
            Some(result) => Ok(result),
            // The target request may not be completed.
            None => {
                // Peek the underlying io_uring.
                match self.io_uring.wait() {
                    // Some operation has completed.
                    Ok((other_request_id, size)) => {
                        let msg: Box<liburing::msghdr> = unsafe { Box::from_raw(other_request_id) };
                        let _: Box<liburing::iovec> = unsafe { Box::from_raw(msg.msg_iov) };
                        let addr: Option<SocketAddrV4> = if msg.msg_name.is_null() {
                            None
                        } else {
                            let saddr: *const SockAddr = msg.msg_name as *const SockAddr;
                            Some(linux::sockaddr_to_socketaddrv4(unsafe { &*saddr }))
                        };

                        // This is not the request that we are waiting for.
                        if request_id.0 != other_request_id {
                            let other_request_id: RequestId = RequestId(other_request_id);
                            if self.pending.remove(&other_request_id) {
                                self.completed.insert(other_request_id, (addr, size));
                            } else {
                                warn!("spurious event?");
                            }
                        }

                        // Done.
                        Ok((addr, size))
                    },
                    // Something bad has happened.
                    Err(e) => Err(e),
                }
            },
        }
    }

    /// Inserts the `coroutine` named `task_name` into the scheduler.
    pub fn insert_coroutine(&mut self, task_name: &str, coroutine: Pin<Box<Operation>>) -> Result<TaskHandle, Fail> {
        let task: OperationTask = OperationTask::new(task_name.to_string(), coroutine);
        match self.scheduler.insert(task) {
            Some(handle) => Ok(handle),
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for IoUring Runtime
impl MemoryRuntime for IoUringRuntime {}

impl Default for SharedIoUringRuntime {
    /// Creates an I/O user ring runtime.
    fn default() -> Self {
        let io_uring: IoUring = IoUring::new(CATCOLLAR_NUM_RINGS).expect("cannot create io_uring");
        Self(SharedObject::<IoUringRuntime>::new(IoUringRuntime {
            scheduler: Scheduler::default(),
            io_uring: io_uring,
            pending: HashSet::new(),
            completed: HashMap::new(),
        }))
    }
}

impl Deref for SharedIoUringRuntime {
    type Target = IoUringRuntime;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedIoUringRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
