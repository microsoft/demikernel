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
    },
};
use ::std::{
    future::Future,
    mem,
    net::SocketAddrV4,
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
#[derive(Debug)]
pub struct PushFuture {
    // Underlying file descriptor.
    fd: RawFd,
    /// Buffer to send.
    buf: DemiBuffer,
    /// Destination address.
    sockaddr: Option<libc::sockaddr_in>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Push Operation Descriptors
impl PushFuture {
    /// Creates a descriptor for a pushto operation.
    pub fn new(fd: RawFd, buf: DemiBuffer, addr: Option<SocketAddrV4>) -> Self {
        let sockaddr: Option<libc::sockaddr_in> = if let Some(addr) = addr {
            Some(linux::socketaddrv4_to_sockaddr_in(&addr))
        } else {
            None
        };

        Self { fd, buf, sockaddr }
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
            let (addr, addrlen): (*const libc::sockaddr, u32) = if self_.sockaddr.is_some() {
                (
                    (self_.sockaddr.as_ref().unwrap() as *const libc::sockaddr_in) as *const libc::sockaddr,
                    mem::size_of_val(&self_.sockaddr.unwrap()) as u32,
                )
            } else {
                (ptr::null(), 0)
            };
            libc::sendto(
                self_.fd,
                (self_.buf.as_ptr() as *const u8) as *const libc::c_void,
                self_.buf.len(),
                libc::MSG_DONTWAIT,
                addr,
                addrlen,
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
