// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod accept;
pub mod connect;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    accept::AcceptFuture,
    connect::ConnectFuture,
};
use super::DuplexPipe;
use crate::{
    runtime::{
        fail::Fail,
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
    net::SocketAddrV4,
    pin::Pin,
    rc::Rc,
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
    Accept((QDesc, (SocketAddrV4, Rc<DuplexPipe>))),
    Connect((SocketAddrV4, Rc<DuplexPipe>)),
    Failed(Fail),
}

/// Operations Descriptor
pub enum Operation {
    /// Accept operation.
    Accept(QDesc, FutureResult<AcceptFuture>),
    /// Connection operation
    Connect(QDesc, FutureResult<ConnectFuture>),
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Operation Descriptor
impl Operation {
    /// Gets the [OperationResult] output by the target [Operation].
    pub fn get_result(self) -> (QDesc, OperationResult) {
        match self {
            // Accept operation.
            Operation::Accept(
                qd,
                FutureResult {
                    future: _,
                    done: Some(Ok((new_qd, remote, duplex_pipe))),
                },
            ) => (new_qd, OperationResult::Accept((qd, (remote, duplex_pipe)))),
            Operation::Accept(
                qd,
                FutureResult {
                    future: _,
                    done: Some(Err(e)),
                },
            ) => (qd, OperationResult::Failed(e)),

            // Connect operation.
            Operation::Connect(
                qd,
                FutureResult {
                    future: _,
                    done: Some(Ok((remote, duplex_pipe))),
                },
            ) => (qd, OperationResult::Connect((remote, duplex_pipe))),
            Operation::Connect(
                qd,
                FutureResult {
                    future: _,
                    done: Some(Err(e)),
                },
            ) => (qd, OperationResult::Failed(e)),

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
            Operation::Accept(_, ref mut f) => Future::poll(Pin::new(f), ctx),
            Operation::Connect(_, ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<(QDesc, AcceptFuture)> for Operation {
    fn from((qd, f): (QDesc, AcceptFuture)) -> Self {
        Operation::Accept(qd, FutureResult::new(f, None))
    }
}

/// From Trait Implementation for Operation Descriptors
impl From<(QDesc, ConnectFuture)> for Operation {
    fn from((qd, f): (QDesc, ConnectFuture)) -> Self {
        Operation::Connect(qd, FutureResult::new(f, None))
    }
}
