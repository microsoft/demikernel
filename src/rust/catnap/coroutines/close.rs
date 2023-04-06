// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::fail::Fail,
    scheduler::Yielder,
};
use ::std::os::unix::prelude::RawFd;

/// This function calls close on a file descriptor until it is closed successfully.
pub async fn close_coroutine(fd: RawFd, yielder: Yielder) -> Result<(), Fail> {
    loop {
        match unsafe { libc::close(fd) } {
            // Operation completed.
            stats if stats == 0 => {
                trace!("socket closed fd={:?}", fd);
                return Ok(());
            },
            // Operation not completed, thus parse errno to find out what happened.
            _ => {
                let errno: libc::c_int = unsafe { *libc::__errno_location() };

                // Operation was interrupted, retry?
                if errno == libc::EINTR {
                    debug!("close interruptted fd={:?}", fd);
                    // Operation in progress. Check if cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                }
                // Operation failed.
                else {
                    let cause: String = format!("close(): operation failed (errno={:?})", errno);
                    error!("{}", cause);
                    return Err(Fail::new(errno, &cause));
                }
            },
        }
    }
}
