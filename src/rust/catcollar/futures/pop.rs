// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    catcollar::{
        runtime::RequestId,
        IoUringRuntime,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};
use ::std::{
    future::Future,
    net::SocketAddrV4,
    os::fd::RawFd,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Pop Operation Descriptor
pub struct PopFuture {
    /// Underlying runtime.
    rt: IoUringRuntime,
    /// Associated file descriptor.
    fd: RawFd,
    /// Associated receive buffer.
    buf: DemiBuffer,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Creates a descriptor for a pop operation.
    pub fn new(rt: IoUringRuntime, fd: RawFd, buf: DemiBuffer) -> Self {
        Self { rt, fd, buf }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Pop Operation Descriptors
impl Future for PopFuture {
    type Output = Result<(Option<SocketAddrV4>, DemiBuffer), Fail>;

    /// Polls the underlying pop operation.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        let request_id: RequestId = self_.rt.pop(self_.fd, self_.buf.clone())?;
        match self_.rt.peek(request_id) {
            // Operation completed.
            Ok((addr, size)) if size >= 0 => {
                trace!("data received ({:?} bytes)", size);
                let trim_size: usize = self_.buf.len() - (size as usize);
                let mut buf: DemiBuffer = self_.buf.clone();
                buf.trim(trim_size)?;
                Poll::Ready(Ok((addr, buf)))
            },
            // Operation not completed, thus parse errno to find out what happened.
            Ok((None, size)) if size < 0 => {
                let errno: i32 = -size;
                // Operation in progress.
                if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN {
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                // Operation failed.
                else {
                    let message: String = format!("pus(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
            // Operation failed.
            Err(e) => {
                warn!("pop failed ({:?})", e);
                Poll::Ready(Err(e))
            },
            // Should not happen.
            _ => panic!("pop failed: unknown error"),
        }
    }
}
