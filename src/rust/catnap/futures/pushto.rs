// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    pal::linux,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        QDesc,
    },
};
use ::std::{
    future::Future,
    mem,
    net::SocketAddrV4,
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

/// Pushto Operation Descriptor
pub struct PushtoFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Destination address.
    sockaddr: libc::sockaddr_in,
    // Underlying file descriptor.
    fd: RawFd,
    /// Buffer to send.
    buf: DemiBuffer,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pushto Operation Descriptors
impl PushtoFuture {
    /// Creates a descriptor for a pushto operation.
    pub fn new(qd: QDesc, fd: RawFd, addr: SocketAddrV4, buf: DemiBuffer) -> Self {
        Self {
            qd,
            sockaddr: linux::socketaddrv4_to_sockaddr_in(&addr),
            fd,
            buf,
        }
    }

    /// Returns the queue descriptor associated to the target [PushtoFuture].
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
        match unsafe {
            libc::sendto(
                self_.fd,
                (self_.buf.as_ptr() as *const u8) as *const libc::c_void,
                self_.buf.len(),
                libc::MSG_DONTWAIT,
                (&self_.sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
                mem::size_of_val(&self_.sockaddr) as u32,
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
                    let message: String = format!("pushto(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
        }
    }
}
