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

/// Accept Operation Descriptor
pub struct AcceptFuture {
    /// Underlying file descriptor.
    fd: RawFd,
    /// Socket address of accept connection.
    saddr: libc::sockaddr,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Accept Operation Descriptors
impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(fd: RawFd) -> Self {
        Self {
            fd,
            saddr: unsafe { mem::zeroed() },
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Accept Operation Descriptors
impl Future for AcceptFuture {
    type Output = Result<(RawFd, SocketAddrV4), Fail>;

    /// Polls the underlying accept operation.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut AcceptFuture = self.get_mut();
        match unsafe {
            let mut address_len: libc::socklen_t = mem::size_of::<libc::sockaddr_in>() as u32;
            libc::accept(self_.fd, &mut self_.saddr as *mut libc::sockaddr, &mut address_len)
        } {
            // Operation completed.
            new_fd if new_fd >= 0 => {
                trace!("connection accepted ({:?})", new_fd);

                // Set socket options.
                unsafe {
                    if linux::set_tcp_nodelay(new_fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set TCP_NONDELAY option (errno={:?})", errno);
                    }
                    if linux::set_nonblock(new_fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set O_NONBLOCK option (errno={:?})", errno);
                    }
                    if linux::set_so_reuseport(new_fd) != 0 {
                        let errno: libc::c_int = *libc::__errno_location();
                        warn!("cannot set SO_REUSEPORT option (errno={:?})", errno);
                    }
                }

                let addr: SocketAddrV4 = linux::sockaddr_to_socketaddrv4(&self_.saddr);
                Poll::Ready(Ok((new_fd, addr)))
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
                    let message: String = format!("accept(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
        }
    }
}
