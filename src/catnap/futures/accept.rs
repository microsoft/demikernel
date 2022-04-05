// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::nix::{
    errno::Errno,
    sys::socket,
};
use ::runtime::{
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

/// Accept Operation Descriptor
pub struct AcceptFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Underlying file descriptor.
    fd: RawFd,
    /// Queue descriptor of incoming connection.
    new_qd: QDesc,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Accept Operation Descriptors
impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(qd: QDesc, fd: RawFd, new_qd: QDesc) -> Self {
        Self { qd, fd, new_qd }
    }

    /// Returns the queue descriptor associated to the target [AcceptFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }

    /// Returns the new queue descriptor associated to the target [AcceptFuture].
    pub fn get_new_qd(&self) -> QDesc {
        self.new_qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Accept Operation Descriptors
impl Future for AcceptFuture {
    type Output = Result<RawFd, Fail>;

    /// Polls the target [AcceptFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &AcceptFuture = self.get_mut();
        match socket::accept(self_.fd as i32) {
            // Operation completed.
            Ok(new_fd) => {
                trace!("connection accepted ({:?})", new_fd);
                Poll::Ready(Ok(new_fd))
            },
            // Operation in progress.
            Err(e) if e == Errno::EWOULDBLOCK || e == Errno::EAGAIN => {
                trace!("listening for connections ({:?})", e);
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Operation failed.
            Err(e) => {
                warn!("failed to accept connection ({:?})", e);
                Poll::Ready(Err(Fail::new(e as i32, "operation failed")))
            },
        }
    }
}
