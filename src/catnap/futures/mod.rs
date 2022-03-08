// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Exports
//==============================================================================

pub mod accept;
pub mod connect;
pub mod pop;
pub mod push;
pub mod pushto;

//==============================================================================
// Imports
//==============================================================================

use self::{
    accept::AcceptFuture,
    connect::ConnectFuture,
    pop::PopFuture,
    push::PushFuture,
    pushto::PushtoFuture,
};
use ::catwalk::{
    FutureResult,
    SchedulerFuture,
};
use ::runtime::{
    fail::Fail,
    memory::Bytes,
    network::types::{
        Ipv4Addr,
        Port16,
    },
    QDesc,
};
use ::std::{
    any::Any,
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

/// Operation Result
pub enum OperationResult {
    Connect,
    Accept(RawFd),
    Push,
    Pop(Option<(Ipv4Addr, Port16)>, Bytes),
    Failed(Fail),
}

/// Operations Descriptor
pub enum FutureOperation {
    Accept(FutureResult<AcceptFuture>),
    Connect(FutureResult<ConnectFuture>),
    Push(FutureResult<PushFuture>),
    Pushto(FutureResult<PushtoFuture>),
    Pop(FutureResult<PopFuture>),
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Operation Descriptor
impl FutureOperation {
    pub fn expect_result(self) -> (QDesc, OperationResult) {
        match self {
            // Accept operation.
            FutureOperation::Accept(FutureResult {
                future,
                done: Some(Ok(fd)),
            }) => (future.get_qd(), OperationResult::Accept(fd)),
            FutureOperation::Accept(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            // Connect operation.
            FutureOperation::Connect(FutureResult {
                future,
                done: Some(Ok(())),
            }) => (future.get_qd(), OperationResult::Connect),
            FutureOperation::Connect(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            // Push operation.
            FutureOperation::Push(FutureResult {
                future,
                done: Some(Ok(())),
            }) => (future.get_qd(), OperationResult::Push),
            FutureOperation::Push(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            // Pushto operation.
            FutureOperation::Pushto(FutureResult {
                future,
                done: Some(Ok(())),
            }) => (future.get_qd(), OperationResult::Push),
            FutureOperation::Pushto(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            // Pop operation.
            FutureOperation::Pop(FutureResult {
                future,
                done: Some(Ok(buf)),
            }) => (future.get_qd(), OperationResult::Pop(None, buf)),
            FutureOperation::Pop(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            _ => panic!("future not ready"),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Scheduler Future Trait Implementation for Operation Descriptors
impl SchedulerFuture for FutureOperation {
    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn get_future(&self) -> &dyn Future<Output = ()> {
        todo!()
    }
}

/// Future Trait Implementation for Operation Descriptors
impl Future for FutureOperation {
    type Output = ();

    /// Polls the target [FutureOperation].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        trace!("polling...");
        match self.get_mut() {
            FutureOperation::Accept(ref mut f) => Future::poll(Pin::new(f), ctx),
            FutureOperation::Connect(ref mut f) => Future::poll(Pin::new(f), ctx),
            FutureOperation::Push(ref mut f) => Future::poll(Pin::new(f), ctx),
            FutureOperation::Pushto(ref mut f) => Future::poll(Pin::new(f), ctx),
            FutureOperation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<AcceptFuture> for FutureOperation {
    fn from(f: AcceptFuture) -> Self {
        FutureOperation::Accept(FutureResult::new(f, None))
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<ConnectFuture> for FutureOperation {
    fn from(f: ConnectFuture) -> Self {
        FutureOperation::Connect(FutureResult::new(f, None))
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<PushFuture> for FutureOperation {
    fn from(f: PushFuture) -> Self {
        FutureOperation::Push(FutureResult::new(f, None))
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<PushtoFuture> for FutureOperation {
    fn from(f: PushtoFuture) -> Self {
        FutureOperation::Pushto(FutureResult::new(f, None))
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<PopFuture> for FutureOperation {
    fn from(f: PopFuture) -> Self {
        FutureOperation::Pop(FutureResult::new(f, None))
    }
}
