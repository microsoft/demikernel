// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    pal::linux,
    runtime::{
        fail::Fail,
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

/// Accept Operation Descriptor
pub struct AcceptFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Underlying file descriptor.
    fd: RawFd,
    /// Queue descriptor of incoming connection.
    new_qd: QDesc,
    /// Socket address of accept connection.
    sockaddr: libc::sockaddr_in,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Accept Operation Descriptors
impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(qd: QDesc, fd: RawFd, new_qd: QDesc) -> Self {
        Self {
            qd,
            fd,
            new_qd,
            sockaddr: unsafe { mem::zeroed() },
        }
    }

    /// Returns the queue descriptor associated to the target accept operation
    /// descriptor.
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }

    /// Returns the new queue descriptor of the incoming connection associated
    /// to the target accept operation descriptor.
    pub fn get_new_qd(&self) -> QDesc {
        self.new_qd
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
            libc::accept(
                self_.fd,
                (&mut self_.sockaddr as *mut libc::sockaddr_in) as *mut libc::sockaddr,
                &mut address_len,
            )
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

                let addr: SocketAddrV4 = linux::sockaddr_in_to_socketaddrv4(&self_.sockaddr);
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
