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
    /// Source socket address.
    sockaddr: libc::sockaddr_in,
    /// Receiving buffer.
    buf: DemiBuffer,
    /// Number of bytes to pop.
    size: usize,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Creates a descriptor for a pop operation.
    pub fn new(qd: QDesc, fd: RawFd, size: Option<usize>) -> Self {
        let size: usize = size.unwrap_or(POP_SIZE);
        let buf: DemiBuffer = DemiBuffer::new(size as u16);
        Self {
            qd,
            fd,
            sockaddr: unsafe { mem::zeroed() },
            buf,
            size,
        }
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
    type Output = Result<(Option<SocketAddrV4>, DemiBuffer), Fail>;

    /// Polls the target [PopFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        let size: usize = self_.size;
        match unsafe {
            let mut addrlen: libc::socklen_t = mem::size_of::<libc::sockaddr_in>() as u32;
            libc::recvfrom(
                self_.fd,
                (self_.buf.as_mut_ptr() as *mut u8) as *mut libc::c_void,
                self_.buf.len(),
                libc::MSG_DONTWAIT,
                (&mut self_.sockaddr as *mut libc::sockaddr_in) as *mut libc::sockaddr,
                &mut addrlen as *mut u32,
            )
        } {
            // Operation completed.
            nbytes if nbytes >= 0 => {
                trace!("data received ({:?}/{:?} bytes)", nbytes, size);
                self_.buf.trim(size - nbytes as usize)?;
                let addr: SocketAddrV4 = linux::sockaddr_in_to_socketaddrv4(&self_.sockaddr);
                return Poll::Ready(Ok((Some(addr), self_.buf.clone())));
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
                    let message: String = format!("pop(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Poll::Ready(Err(Fail::new(errno, &message)));
                }
            },
        }
    }
}
