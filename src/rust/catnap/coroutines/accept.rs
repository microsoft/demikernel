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

/// This function polls accept on a listening socket until it receives a new accepted connection back.
pub async fn accept_coroutine(fd: RawFd, yielder: Yielder) -> Result<(RawFd, SocketAddrV4), Fail> {
    let mut saddr: libc::sockaddr = unsafe { mem::zeroed() };
    let mut address_len: libc::socklen_t = mem::size_of::<libc::sockaddr_in>() as u32;

    loop {
        match unsafe { libc::accept(fd, &mut saddr as *mut libc::sockaddr, &mut address_len) } {
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

                let addr: SocketAddrV4 = linux::sockaddr_to_socketaddrv4(&saddr);
                return Ok((new_fd, addr));
            },
            _ => {
                // Operation not completed, thus parse errno to find out what happened.
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                if errno == libc::EWOULDBLOCK || errno == libc::EAGAIN {
                    // Operation in progress. Check if cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                } else {
                    // Operation failed.
                    let message: String = format!("accept(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
        }
    }
}
