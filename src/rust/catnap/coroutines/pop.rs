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
};

//==============================================================================
// Constants
//==============================================================================

/// Maximum Size for a Pop Operation
const POP_SIZE: usize = 9216;

/// This function polls read until it receives some data or an error and then returns the data to pop.
pub async fn pop_coroutine(
    fd: RawFd,
    size: Option<usize>,
    yielder: Yielder,
) -> Result<(Option<SocketAddrV4>, DemiBuffer), Fail> {
    let size: usize = size.unwrap_or(POP_SIZE);
    let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
    let mut sockaddr: libc::sockaddr_in = unsafe { mem::zeroed() };
    let mut addrlen: libc::socklen_t = mem::size_of::<libc::sockaddr_in>() as u32;

    // Check that we allocated a DemiBuffer that is big enough.
    assert!(buf.len() == size);

    // Poll recv.
    loop {
        match unsafe {
            libc::recvfrom(
                fd,
                (buf.as_mut_ptr() as *mut u8) as *mut libc::c_void,
                size,
                libc::MSG_DONTWAIT,
                (&mut sockaddr as *mut libc::sockaddr_in) as *mut libc::sockaddr,
                &mut addrlen as *mut u32,
            )
        } {
            // Operation completed.
            nbytes if nbytes >= 0 => {
                trace!("data received ({:?}/{:?} bytes)", nbytes, POP_SIZE);
                buf.trim(size - nbytes as usize)?;
                let addr: SocketAddrV4 = linux::sockaddr_in_to_socketaddrv4(&sockaddr);
                return Ok((Some(addr), buf.clone()));
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
                    let message: String = format!("pop(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
        }
    }
}
