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
    scheduler::Yielder,
};
use ::std::{
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
    ptr,
};

/// This function polls write until all the data in the push is sent.
pub async fn push_coroutine(
    fd: RawFd,
    buf: DemiBuffer,
    addr: Option<SocketAddrV4>,
    yielder: Yielder,
) -> Result<(), Fail> {
    let saddr: Option<libc::sockaddr> = if let Some(addr) = addr.as_ref() {
        Some(linux::socketaddrv4_to_sockaddr(addr))
    } else {
        None
    };

    // Note that we use references here, so as we don't end up constructing a dangling pointer.
    let (saddr_ptr, sockaddr_len) = if let Some(saddr_ref) = saddr.as_ref() {
        (
            saddr_ref as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    } else {
        (ptr::null(), 0)
    };
    loop {
        match unsafe {
            libc::sendto(
                fd,
                (buf.as_ptr() as *const u8) as *const libc::c_void,
                buf.len(),
                libc::MSG_DONTWAIT,
                saddr_ptr,
                sockaddr_len,
            )
        } {
            // Operation completed.
            nbytes if nbytes >= 0 => {
                trace!("data pushed ({:?}/{:?} bytes)", nbytes, buf.len());
                return Ok(());
            },

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation in progress.
                if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN {
                    // Operation in progress. Check if cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                }
                // Operation failed.
                else {
                    let message: String = format!("pushto(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
        }
    }
}
