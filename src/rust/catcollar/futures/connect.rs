// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    pal::linux,
    runtime::fail::Fail,
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

/// Connect Operation Descriptor
pub struct ConnectFuture {
    // Underlying file descriptor.
    fd: RawFd,
    /// Connect address.
    sockaddr: libc::sockaddr_in,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Connect Operation Descriptors
impl ConnectFuture {
    /// Creates a descriptor for a connect operation.
    pub fn new(fd: RawFd, addr: SocketAddrV4) -> Self {
        Self {
            fd,
            sockaddr: linux::socketaddrv4_to_sockaddr_in(&addr),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Connect Operation Descriptors
impl Future for ConnectFuture {
    type Output = Result<(), Fail>;

    /// Polls the underlying connect operation.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut ConnectFuture = self.get_mut();
        match unsafe {
            libc::connect(
                self_.fd,
                (&self_.sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
                mem::size_of_val(&self_.sockaddr) as u32,
            )
        } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("connection established ({:?})", self_.sockaddr);
                Poll::Ready(Ok(()))
            },

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation in progress.
                if errno == libc::EINPROGRESS || errno == libc::EALREADY {
                    trace!("connect in progress ({:?})", errno);
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                // Operation failed.
                else {
                    let message: String = format!("connect(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
        }
    }
}
