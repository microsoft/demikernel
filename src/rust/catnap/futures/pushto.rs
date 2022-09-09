// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    memory::Buffer,
    QDesc,
};
use ::nix::{
    errno::Errno,
    sys::socket::{
        self,
        MsgFlags,
        SockaddrStorage,
    },
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

/// Pushto Operation Descriptor
pub struct PushtoFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Destination address.
    addr: SockaddrStorage,
    // Underlying file descriptor.
    fd: RawFd,
    /// Buffer to send.
    buf: Buffer,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pushto Operation Descriptors
impl PushtoFuture {
    /// Creates a descriptor for a pushto operation.
    pub fn new(qd: QDesc, fd: RawFd, addr: SockaddrStorage, buf: Buffer) -> Self {
        Self { qd, addr, fd, buf }
    }

    /// Returns the queue descriptor associated to the target [PushtoFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Pushto Operation Descriptors
impl Future for PushtoFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [PushtoFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PushtoFuture = self.get_mut();
        match socket::sendto(self_.fd, &self_.buf[..], &self_.addr, MsgFlags::empty()) {
            // Operation completed.
            Ok(nbytes) => {
                trace!("data pushed ({:?}/{:?} bytes)", nbytes, self_.buf.len());
                Poll::Ready(Ok(()))
            },
            // Operation in progress.
            Err(e) if e == Errno::EWOULDBLOCK || e == Errno::EAGAIN => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Error.
            Err(e) => {
                warn!("push failed ({:?})", e);
                Poll::Ready(Err(Fail::new(e as i32, "operation failed")))
            },
        }
    }
}
