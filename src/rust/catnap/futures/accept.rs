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
    QDesc,
};
use ::std::{
    future::Future,
    mem::size_of_val,
    os::unix::prelude::RawFd,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};
use libc::{
    c_void,
    socklen_t,
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
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Accept Operation Descriptors
impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(qd: QDesc, fd: RawFd, new_qd: QDesc) -> Self {
        Self { qd, fd, new_qd }
    }

    /// Returns the queue descriptor associated to the target [AcceptFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }

    /// Returns the new queue descriptor associated to the target [AcceptFuture].
    pub fn get_new_qd(&self) -> QDesc {
        self.new_qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Accept Operation Descriptors
impl Future for AcceptFuture {
    type Output = Result<RawFd, Fail>;

    /// Polls the target [AcceptFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &AcceptFuture = self.get_mut();
        match socket::accept(self_.fd as i32) {
            // Operation completed.
            Ok(new_fd) => {
                trace!("connection accepted ({:?})", new_fd);

                // Set async options in socket.
                unsafe {
                    if set_tcp_nodelay(new_fd) != 0 {
                        warn!("cannot set TCP_NONDELAY option");
                    }
                    if set_nonblock(new_fd) != 0 {
                        warn!("cannot set NONBLOCK option");
                    }
                }

                Poll::Ready(Ok(new_fd))
            },
            // Operation in progress.
            Err(e) if e == Errno::EWOULDBLOCK || e == Errno::EAGAIN => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Operation failed.
            Err(e) => {
                warn!("failed to accept connection ({:?})", e);
                Poll::Ready(Err(Fail::new(e as i32, "operation failed")))
            },
        }
    }
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Sets TCP_NODELAY option in a socket.
unsafe fn set_tcp_nodelay(fd: RawFd) -> i32 {
    let value: u32 = 1;
    let value_ptr: *const u32 = &value as *const u32;
    let option_len: socklen_t = size_of_val(&value) as socklen_t;
    libc::setsockopt(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_NODELAY,
        value_ptr as *const c_void,
        option_len,
    )
}

/// Sets NONBLOCK option in a socket.
unsafe fn set_nonblock(fd: RawFd) -> i32 {
    // Get file flags.
    let mut flags: i32 = libc::fcntl(fd, libc::F_GETFL);
    if flags == -1 {
        warn!("failed to get flags for new socket");
        return -1;
    }

    // Set file flags.
    flags |= libc::O_NONBLOCK;
    libc::fcntl(fd, libc::F_SETFL, flags, 1)
}
