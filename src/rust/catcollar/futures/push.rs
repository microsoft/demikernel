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

/// Push Operation Descriptor
pub struct PushFuture {
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

/// Associate Functions for Push Operation Descriptors
impl PushFuture {
    /// Creates a descriptor for a push operation.
    pub fn new(rt: IoUringRuntime, fd: RawFd, buf: DemiBuffer) -> Self {
        Self { rt, fd, buf }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Push Operation Descriptors
impl Future for PushFuture {
    type Output = Result<(), Fail>;

    /// Polls the underlying push operation.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PushFuture = self.get_mut();
        let request_id: RequestId = self_.rt.push(self_.fd, self_.buf.clone())?;
        match self_.rt.peek(request_id) {
            // Operation completed.
            Ok((_, size)) if size >= 0 => {
                trace!("data pushed ({:?} bytes)", size);
                Poll::Ready(Ok(()))
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
                    let message: String = format!("push(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
            // Operation failed.
            Err(e) => {
                warn!("push failed ({:?})", e);
                Poll::Ready(Err(e))
            },
            // Should not happen.
            _ => panic!("push failed: unknown error"),
        }
    }
}
