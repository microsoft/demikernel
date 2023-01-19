// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod memory;
mod network;

//==============================================================================
// Imports
//==============================================================================

use super::iouring::IoUring;
use crate::{
    pal::linux,
    runtime::{
        fail::Fail,
        liburing,
        memory::DemiBuffer,
        Runtime,
    },
    scheduler::scheduler::Scheduler,
};
use ::std::{
    cell::RefCell,
    collections::{
        HashMap,
        HashSet,
    },
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
    rc::Rc,
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
#[derive(Clone)]
pub struct IoUringRuntime {
    /// Scheduler
    pub scheduler: Scheduler,
    /// Underlying io_uring.
    io_uring: Rc<RefCell<IoUring>>,
    /// Pending requests.
    pending: HashSet<RequestId>,
    /// Completed requests.
    completed: HashMap<RequestId, i32>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for I/O User Ring Runtime
impl IoUringRuntime {
    /// Creates an I/O user ring runtime.
    pub fn new() -> Self {
        let io_uring: IoUring = IoUring::new(CATCOLLAR_NUM_RINGS).expect("cannot create io_uring");
        Self {
            scheduler: Scheduler::default(),
            io_uring: Rc::new(RefCell::new(io_uring)),
            pending: HashSet::new(),
            completed: HashMap::new(),
        }
    }

    /// Pushes a buffer to the target I/O user ring.
    pub fn push(&mut self, sockfd: RawFd, buf: DemiBuffer) -> Result<RequestId, Fail> {
        let msg_ptr: *const liburing::msghdr = self.io_uring.borrow_mut().push(sockfd, buf)?;
        let request_id: RequestId = RequestId(msg_ptr);
        self.pending.insert(request_id);
        Ok(request_id)
    }

    /// Pushes a buffer to the target I/O user ring.
    pub fn pushto(&mut self, sockfd: i32, addr: SocketAddrV4, buf: DemiBuffer) -> Result<RequestId, Fail> {
        let msg_ptr: *const liburing::msghdr = self.io_uring.borrow_mut().pushto(sockfd, addr, buf)?;
        let request_id: RequestId = RequestId(msg_ptr);
        self.pending.insert(request_id);
        Ok(request_id)
    }

    /// Pops a buffer from the target I/O user ring.
    pub fn pop(&mut self, sockfd: RawFd, buf: DemiBuffer) -> Result<RequestId, Fail> {
        let msg_ptr: *const liburing::msghdr = self.io_uring.borrow_mut().pop(sockfd, buf)?;
        let request_id: RequestId = RequestId(msg_ptr);
        self.pending.insert(request_id);
        Ok(request_id)
    }

    /// Peeks for the completion of an operation in the target I/O user ring.
    pub fn peek(&mut self, request_id: RequestId) -> Result<(Option<SocketAddrV4>, Option<i32>), Fail> {
        // Check if pending request has completed.
        match self.completed.remove(&request_id) {
            // The target request has already completed.
            Some(size) => {
                let msg: Rc<liburing::msghdr> = unsafe { Rc::from_raw(request_id.0) };
                let _: Box<liburing::iovec> = unsafe { Box::from_raw(msg.msg_iov) };
                let addr: Option<SocketAddrV4> = if msg.msg_name.is_null() {
                    None
                } else {
                    let saddr: *const libc::sockaddr = msg.msg_name as *const libc::sockaddr;
                    let sin: libc::sockaddr_in =
                        unsafe { *mem::transmute::<*const libc::sockaddr, *const libc::sockaddr_in>(saddr) };
                    Some(linux::sockaddr_in_to_socketaddrv4(&sin))
                };

                // Done.
                Ok((addr, Some(size)))
            },
            // The target request may not be completed.
            None => {
                // Peek the underlying io_uring.
                match self.io_uring.borrow_mut().wait() {
                    // Some operation has completed.
                    Ok((other_request_id, size)) => {
                        // This is not the request that we are waiting for.
                        if request_id.0 != other_request_id {
                            let other_request_id: RequestId = RequestId(other_request_id);
                            match self.pending.remove(&other_request_id) {
                                true => self.completed.insert(other_request_id, size),
                                false => None,
                            };
                            return Ok((None, None));
                        }
                        let msg: Rc<liburing::msghdr> = unsafe { Rc::from_raw(request_id.0) };
                        let _: Box<liburing::iovec> = unsafe { Box::from_raw(msg.msg_iov) };
                        let addr: Option<SocketAddrV4> = if msg.msg_name.is_null() {
                            None
                        } else {
                            let saddr: *const libc::sockaddr = msg.msg_name as *const libc::sockaddr;
                            let sin: libc::sockaddr_in =
                                unsafe { *mem::transmute::<*const libc::sockaddr, *const libc::sockaddr_in>(saddr) };
                            Some(linux::sockaddr_in_to_socketaddrv4(&sin))
                        };

                        // Done.
                        Ok((addr, Some(size)))
                    },
                    // Something bad has happened.
                    Err(e) => {
                        match e.errno {
                            // Operation in progress.
                            libc::EAGAIN => Ok((None, None)),
                            // Operation failed.
                            _ => Err(e),
                        }
                    },
                }
            },
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Runtime Trait Implementation for I/O User Ring Runtime
impl Runtime for IoUringRuntime {}
