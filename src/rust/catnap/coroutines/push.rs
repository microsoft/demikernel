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
    scheduler::yield_once,
};
use ::std::{
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
    ptr,
};

/// This function polls write until all the data in the push is sent.
pub async fn push_coroutine(fd: RawFd, buf: DemiBuffer, addr: Option<SocketAddrV4>) -> Result<(), Fail> {
    let dest_addr: Option<libc::sockaddr_in> = if let Some(addr_) = addr.as_ref() {
        Some(linux::socketaddrv4_to_sockaddr_in(addr_))
    } else {
        None
    };

    let (sockaddr_ptr, sockaddr_len) = if let Some(sockaddr) = dest_addr.as_ref() {
        (
            (sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
            mem::size_of_val(sockaddr) as u32,
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
                sockaddr_ptr,
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
                    yield_once().await;
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
