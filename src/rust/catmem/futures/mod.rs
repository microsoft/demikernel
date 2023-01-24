// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod pop;
pub mod push;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    pop::PopFuture,
    push::PushFuture,
};
use crate::{
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        QDesc,
    },
    scheduler::{
        FutureResult,
        SchedulerFuture,
    },
};
use ::std::{
    any::Any,
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Operation Result
pub enum OperationResult {
    Push,
    Pop(DemiBuffer, bool),
    Failed(Fail),
}

/// Operations Descriptor
pub enum Operation {
    /// Push operation
    Push(FutureResult<PushFuture>),
    /// Pop operation.
    Pop(FutureResult<PopFuture>),
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Operation Descriptor
impl Operation {
    /// Gets the [OperationResult] output by the target [Operation].
    pub fn get_result(self) -> (QDesc, OperationResult) {
        match self {
            // Push operation.
            Operation::Push(FutureResult {
                future,
                done: Some(Ok(())),
            }) => (future.get_qd(), OperationResult::Push),
            Operation::Push(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            // Pop operation.
            Operation::Pop(FutureResult {
                future,
                done: Some(Ok((buf, eof))),
            }) => (future.get_qd(), OperationResult::Pop(buf, eof)),
            Operation::Pop(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),
            _ => panic!("future not ready"),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Scheduler Future Trait Implementation for Operation Descriptors
impl SchedulerFuture for Operation {
    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn get_future(&self) -> &dyn Future<Output = ()> {
        todo!()
    }
}

/// Future Trait Implementation for Operation Descriptors
impl Future for Operation {
    type Output = ();

    /// Polls the target operation.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut() {
            Operation::Push(ref mut f) => Future::poll(Pin::new(f), ctx),
            Operation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<PushFuture> for Operation {
    fn from(f: PushFuture) -> Self {
        Operation::Push(FutureResult::new(f, None))
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<PopFuture> for Operation {
    fn from(f: PopFuture) -> Self {
        Operation::Pop(FutureResult::new(f, None))
    }
}
