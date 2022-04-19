// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::nix::{
    errno::Errno,
    sys::{
        socket,
        socket::SockAddr,
    },
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

/// Connect Operation Descriptor
pub struct ConnectFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    // Underlying file descriptor.
    fd: RawFd,
    /// Destination address.
    addr: SockAddr,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Connect Operation Descriptors
impl ConnectFuture {
    /// Creates a descriptor for a connect operation.
    pub fn new(qd: QDesc, fd: RawFd, addr: SockAddr) -> Self {
        Self { qd, fd, addr }
    }

    /// Returns the queue descriptor associated to the target [ConnectFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Connect Operation Descriptors
impl Future for ConnectFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [ConnectFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut ConnectFuture = self.get_mut();
        match socket::connect(self_.fd as i32, &self_.addr) {
            // Operation completed.
            Ok(_) => {
                trace!("connection established ({:?})", self_.addr);
                Poll::Ready(Ok(()))
            },
            // Operation not ready yet.
            Err(e) if e == Errno::EINPROGRESS || e == Errno::EALREADY => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Operation failed.
            Err(e) => {
                warn!("failed to establish connection ({:?})", e);
                Poll::Ready(Err(Fail::new(e as i32, "operation failed")))
            },
        }
    }
}
