// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::nix::{
    errno::Errno,
    sys::socket,
};
use ::runtime::{
    fail::Fail,
    memory::{
        Buffer,
        DataBuffer,
    },
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
// Constants
//==============================================================================

/// Maximum Size for a Pop Operation
const POP_SIZE: usize = 9216;

//==============================================================================
// Structures
//==============================================================================

/// Pop Operation Descriptor
pub struct PopFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Underlying file descriptor.
    fd: RawFd,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Creates a descriptor for a pop operation.
    pub fn new(qd: QDesc, fd: RawFd) -> Self {
        Self { qd, fd }
    }

    /// Returns the queue descriptor associated to the target [PopFuture].
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

    /// Polls the target [PopFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        let mut bytes: [u8; POP_SIZE] = [0; POP_SIZE];
        match socket::recv(self_.fd, &mut bytes[..], socket::MsgFlags::empty()) {
            // Operation completed.
            Ok(nbytes) => {
                trace!("data received ({:?}/{:?} bytes)", nbytes, POP_SIZE);
                let buf: Buffer = Buffer::Heap(DataBuffer::from_slice(&bytes[0..nbytes]));
                Poll::Ready(Ok(buf))
            },
            // Operation in progress.
            Err(e) if e == Errno::EWOULDBLOCK || e == Errno::EAGAIN => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Error.
            Err(e) => {
                trace!("pop failed ({:?})", e);
                Poll::Ready(Err(Fail::new(e as i32, "operation failed")))
            },
        }
    }
}
