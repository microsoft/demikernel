// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::nix::{
    errno::Errno,
    sys::socket::{
        sendto,
        MsgFlags,
        SockAddr,
    },
};
use ::runtime::{
    fail::Fail,
    memory::Bytes,
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

/// Pushto Operation Descriptor
pub struct PushtoFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Destination address.
    addr: SockAddr,
    // Underlying file descriptor.
    fd: RawFd,
    /// Buffer to send.
    buf: Bytes,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pushto Operation Descriptors
impl PushtoFuture {
    /// Creates a descriptor for a pushto operation.
    pub fn new(qd: QDesc, fd: RawFd, addr: SockAddr, buf: Bytes) -> Self {
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
        match sendto(self_.fd, &self_.buf[..], &self_.addr, MsgFlags::empty()) {
            // Operation completed.
            Ok(_) => {
                trace!("data pushed ({:?} bytes)", self_.buf.len());
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
