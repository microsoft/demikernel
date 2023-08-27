// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    catcollar::futures::retry_errno,
    runtime::fail::Fail,
    scheduler::Yielder,
};
use ::std::os::unix::prelude::RawFd;

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
                if retry_errno(errno) {
                    if let Err(e) = yielder.yield_once().await {
                        let message: String = format!("close(): operation canceled (err={:?})", e);
                        error!("{}", message);
                        return Err(Fail::new(libc::ECANCELED, &message));
                    }
                } else {
                    // Operation failed.
                    let message: String = format!("close(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
        }
    }
}
