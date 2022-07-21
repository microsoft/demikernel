// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod memory;
mod network;
mod scheduler;

//==============================================================================
// Imports
//==============================================================================

use super::iouring::IoUring;
use ::nix::sys::socket::SockaddrStorage;
use ::runtime::{
    fail::Fail,
    memory::Buffer,
    scheduler::Scheduler,
    timer::{
        Timer,
        TimerRc,
    },
    Runtime,
};
use ::std::{
    cell::RefCell,
    collections::HashMap,
    os::unix::prelude::RawFd,
    rc::Rc,
    time::Instant,
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
pub struct RequestId(u64);

/// I/O User Ring Runtime
#[derive(Clone)]
pub struct IoUringRuntime {
    /// Timer.
    timer: TimerRc,
    /// Scheduler
    scheduler: Scheduler,
    /// Underlying io_uring.
    io_uring: Rc<RefCell<IoUring>>,
    /// Pending requests.
    pending: HashMap<RequestId, i32>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for I/O User Ring Runtime
impl IoUringRuntime {
    /// Creates an I/O user ring runtime.
    pub fn new(now: Instant) -> Self {
        let io_uring: IoUring = IoUring::new(CATCOLLAR_NUM_RINGS).expect("cannot create io_uring");
        Self {
            timer: TimerRc(Rc::new(Timer::new(now))),
            scheduler: Scheduler::default(),
            io_uring: Rc::new(RefCell::new(io_uring)),
            pending: HashMap::new(),
        }
    }

    /// Pushes a buffer to the target I/O user ring.
    pub fn push(&mut self, sockfd: RawFd, buf: Buffer) -> Result<RequestId, Fail> {
        let request_id: RequestId = self.io_uring.borrow_mut().push(sockfd, buf)?.into();
        Ok(request_id)
    }

    /// Pops a buffer from the target I/O user ring.
    pub fn pop(&mut self, sockfd: RawFd, buf: Buffer) -> Result<RequestId, Fail> {
        let request_id: RequestId = self.io_uring.borrow_mut().pop(sockfd, buf)?.into();
        Ok(request_id)
    }

    /// Peeks for the completion of an operation in the target I/O user ring.
    pub fn peek(&mut self, request_id: RequestId) -> Result<Option<i32>, Fail> {
        // Check if pending request has completed.
        match self.pending.remove(&request_id) {
            // The target request has already completed.
            Some(size) => Ok(Some(size)),
            // The target request may not be completed.
            None => {
                // Peek the underlying io_uring.
                match self.io_uring.borrow_mut().wait() {
                    // Some operation has completed.
                    Ok((other_request_id, size)) => {
                        // This is not the request that we are waiting for.
                        // So, queue this request and return.
                        if request_id != other_request_id.into() {
                            self.pending.insert(other_request_id.into(), size);
                            return Ok(None);
                        }

                        // Done.
                        Ok(Some(size))
                    },
                    // Something bad has happened.
                    Err(e) => {
                        match e.errno {
                            // Operation in progress.
                            libc::EAGAIN => Ok(None),
                            // Operation failed.
                            _ => Err(e),
                        }
                    },
                }
            },
        }
    }

    /// Pushes a buffer to the target I/O user ring.
    pub fn pushto(&self, sockfd: i32, addr: SockaddrStorage, buf: Buffer) -> Result<RequestId, Fail> {
        let request_id: RequestId = self.io_uring.borrow_mut().pushto(sockfd, addr, buf)?.into();
        Ok(request_id)
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Runtime Trait Implementation for I/O User Ring Runtime
impl Runtime for IoUringRuntime {}

/// Conversion Trait Implementation for Request IDs
impl From<u64> for RequestId {
    fn from(val: u64) -> Self {
        RequestId(val)
    }
}
