// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    QDesc,
};
use ::std::{
    future::Future,
    os::unix::prelude::RawFd,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Close Operation Descriptor
pub struct CloseFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    // Underlying file descriptor.
    fd: RawFd,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Close Operation Descriptors
impl CloseFuture {
    /// Creates a descriptor for a close operation.
    pub fn new(qd: QDesc, fd: RawFd) -> Self {
        Self { qd, fd }
    }

    /// Returns the queue descriptor associated to the target [ConnectFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Close Operation Descriptors
impl Future for CloseFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [CloseFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut CloseFuture = self.get_mut();
        match unsafe { libc::close(self_.fd) } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("socket closed fd={:?}", self_.fd);
                Poll::Ready(Ok(()))
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation was interrupted, retry?
                if errno == libc::EINTR {
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                // Operation failed.
                else {
                    let message: String = format!("close(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
        }
    }
}
