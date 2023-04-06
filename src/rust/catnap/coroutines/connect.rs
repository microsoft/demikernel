// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    pal::linux,
    runtime::fail::Fail,
    scheduler::Yielder,
};
use ::std::{
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
};

/// This function polls connect on a socket file descriptor until the connection is established (or returns an error).
pub async fn connect_coroutine(fd: RawFd, addr: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
    loop {
        let sockaddr: libc::sockaddr_in = linux::socketaddrv4_to_sockaddr_in(&addr);
        match unsafe {
            libc::connect(
                fd,
                (&sockaddr as *const libc::sockaddr_in) as *const libc::sockaddr,
                mem::size_of_val(&sockaddr) as u32,
            )
        } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("connection established ({:?})", addr);
                return Ok(());
            },

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation in progress.
                if errno == libc::EINPROGRESS || errno == libc::EALREADY {
                    // Operation in progress. Check if cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                }
                // Operation failed.
                else {
                    let message: String = format!("connect(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
        }
    }
}
