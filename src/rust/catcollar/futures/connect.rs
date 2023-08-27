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

pub async fn connect_coroutine(fd: RawFd, remote: SocketAddrV4, yielder: Yielder) -> Result<(), Fail> {
    let saddr: SockAddr = linux::socketaddrv4_to_sockaddr(&remote);
    loop {
        match unsafe { libc::connect(fd, &saddr as *const SockAddr, mem::size_of::<SockAddrIn>() as Socklen) } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("connection established (fd={:?})", fd);
                return Ok(());
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation in progress.
                if retry_errno(errno) {
                    if let Err(e) = yielder.yield_once().await {
                        let message: String = format!("connect(): operation canceled (err={:?})", e);
                        error!("{}", message);
                        return Err(Fail::new(libc::ECANCELED, &message));
                    }
                } else {
                    // Operation failed.
                    let message: String = format!("connect(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
        }
    }
}
