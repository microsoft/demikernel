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

/// Push Operation Descriptor
pub struct PushFuture {
    /// Underlying runtime.
    rt: IoUringRuntime,
    /// Associated queue descriptor.
    qd: QDesc,
    /// Associated request.
    request_id: RequestId,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Push Operation Descriptors
impl PushFuture {
    /// Creates a descriptor for a push operation.
    pub fn new(rt: IoUringRuntime, request_id: RequestId, qd: QDesc) -> Self {
        Self { rt, request_id, qd }
    }

    /// Returns the queue descriptor associated to the target push operation descriptor.
    pub fn get_qd(&self) -> QDesc {
        self.qd
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
        match self_.rt.peek(self_.request_id) {
            // Operation completed.
            Ok((_, Some(size))) if size >= 0 => {
                trace!("data pushed ({:?} bytes)", size);
                Poll::Ready(Ok(()))
            },
            // Operation in progress, re-schedule future.
            Ok((None, None)) => {
                trace!("push in progress");
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Underlying asynchronous operation failed.
            Ok((None, Some(size))) if size < 0 => {
                let errno: i32 = -size;
                warn!("push failed ({:?})", errno);
                Poll::Ready(Err(Fail::new(errno, "I/O error")))
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
