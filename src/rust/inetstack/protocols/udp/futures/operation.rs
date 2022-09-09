// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::{
        operations::OperationResult,
        protocols::udp::UdpPopFuture,
    },
    runtime::{
        fail::Fail,
        scheduler::scheduler::FutureResult,
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
// Enumerations
//==============================================================================

/// UDP Operation Descriptor
pub enum UdpOperation {
    /// Pushto operation.
    Pushto(QDesc, Result<(), Fail>),
    /// Pop operation.
    Pop(FutureResult<UdpPopFuture>),
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for UDP Operation Descriptors
impl UdpOperation {
    pub fn get_result(self) -> (QDesc, OperationResult) {
        match self {
            // Pushto operation.
            UdpOperation::Pushto(fd, Ok(())) => (fd, OperationResult::Push),
            UdpOperation::Pushto(fd, Err(e)) => (fd, OperationResult::Failed(e)),

            // Pop operation.
            UdpOperation::Pop(FutureResult {
                future,
                done: Some(Ok((addr, bytes))),
            }) => (future.get_qd(), OperationResult::Pop(Some(addr), bytes)),
            UdpOperation::Pop(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.get_qd(), OperationResult::Failed(e)),

            _ => panic!("UDP Operation not ready"),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future trait implementation for UDP Operation Descriptors
impl Future for UdpOperation {
    type Output = ();

    /// Poll the target UDP operation descritor.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        match self.get_mut() {
            UdpOperation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx),
            UdpOperation::Pushto(..) => Poll::Ready(()),
        }
    }
}
