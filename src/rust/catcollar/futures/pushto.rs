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
        QDesc,
    },
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

/// Pushto Operation Descriptor
pub struct PushtoFuture {
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

/// Associate Functions for Pushto Operation Descriptors
impl PushtoFuture {
    /// Creates a descriptor for a pushto operation.
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

/// Future Trait Implementation for Pushto Operation Descriptors
impl Future for PushtoFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [PushtoFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PushtoFuture = self.get_mut();
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
