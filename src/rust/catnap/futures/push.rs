// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    QDesc,
};
use ::std::{
    future::Future,
    os::unix::prelude::RawFd,
    pin::Pin,
    ptr,
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
    /// Associated queue descriptor.
    qd: QDesc,
    // Underlying file descriptor.
    fd: RawFd,
    /// Buffer to send.
    buf: DemiBuffer,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Push Operation Descriptors
impl PushFuture {
    /// Creates a descriptor for a push operation.
    pub fn new(qd: QDesc, fd: RawFd, buf: DemiBuffer) -> Self {
        Self { qd, fd, buf }
    }

    /// Returns the queue descriptor associated to the target [PushFuture].
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

    /// Polls the target [PushFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PushFuture = self.get_mut();
        match unsafe {
            libc::sendto(
                self_.fd,
                (self_.buf.as_ptr() as *const u8) as *const libc::c_void,
                self_.buf.len(),
                libc::MSG_DONTWAIT,
                ptr::null(),
                0,
            )
        } {
            // Operation completed.
            nbytes if nbytes >= 0 => {
                trace!("data pushed ({:?}/{:?} bytes)", nbytes, self_.buf.len());
                Poll::Ready(Ok(()))
            },

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

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
        }
    }
}
