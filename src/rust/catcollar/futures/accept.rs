// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    catcollar::futures::retry_errno,
    pal::{
        data_structures::{
            SockAddr,
            SockAddrIn,
            Socklen,
        },
        linux,
    },
    runtime::fail::Fail,
    scheduler::Yielder,
};
use ::std::{
    mem,
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
};

pub async fn accept_coroutine(fd: RawFd, yielder: Yielder) -> Result<(RawFd, SocketAddrV4), Fail> {
    // Socket address of accept connection.
    let mut saddr: SockAddr = unsafe { mem::zeroed() };
    let mut address_len: Socklen = mem::size_of::<SockAddrIn>() as u32;

    loop {
        match unsafe { libc::accept(fd, &mut saddr as *mut SockAddr, &mut address_len) } {
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

            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation in progress.
                if retry_errno(errno) {
                    if let Err(e) = yielder.yield_once().await {
                        let message: String = format!("accept(): operation canceled (err={:?})", e);
                        error!("{}", message);
                        return Err(Fail::new(libc::ECANCELED, &message));
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
