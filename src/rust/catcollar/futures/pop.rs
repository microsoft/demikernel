// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::catcollar::{
    runtime::RequestId,
    IoUringRuntime,
};
use ::runtime::{
    fail::Fail,
    memory::Buffer,
    QDesc,
};
use ::std::{
    future::Future,
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
    /// Associated queue descriptor.
    qd: QDesc,
    /// Associated receive buffer.
    buf: Buffer,
    /// Associated request.
    request_id: RequestId,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Creates a descriptor for a pop operation.
    pub fn new(rt: IoUringRuntime, request_id: RequestId, qd: QDesc, buf: Buffer) -> Self {
        Self {
            rt,
            qd,
            buf,
            request_id,
        }
    }

    /// Returns the queue descriptor associated to the target pop operation descriptor.
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Pop Operation Descriptors
impl Future for PopFuture {
    type Output = Result<Buffer, Fail>;

    /// Polls the underlying pop operation.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        match self_.rt.peek(self_.request_id) {
            // Operation completed.
            Ok(Some(size)) if size >= 0 => {
                trace!("data received ({:?} bytes)", size);
                let trim_size: usize = self_.buf.len() - (size as usize);
                let mut buf: Buffer = self_.buf.clone();
                buf.trim(trim_size);
                Poll::Ready(Ok(buf))
            },
            // Operation in progress, re-schedule future.
            Ok(None) => {
                trace!("pop in progress");
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Underlying asynchronous operation failed.
            Ok(Some(size)) if size < 0 => {
                let errno: i32 = -size;
                warn!("pop failed ({:?})", errno);
                Poll::Ready(Err(Fail::new(errno, "I/O error")))
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
