// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    catcollar::{
        futures::retry_errno,
        runtime::RequestId,
        IoUringRuntime,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
    scheduler::Yielder,
};
use ::std::{
    net::SocketAddrV4,
    os::fd::RawFd,
};

pub async fn pushto_coroutine(
    mut rt: IoUringRuntime,
    fd: RawFd,
    addr: SocketAddrV4,
    buf: DemiBuffer,
    yielder: Yielder,
) -> Result<(), Fail> {
    loop {
        let request_id: RequestId = rt.pushto(fd, addr, buf.clone())?;
        match rt.peek(request_id) {
            // Operation completed.
            Ok((_, size)) if size >= 0 => {
                trace!("data pushed ({:?} bytes)", size);
                return Ok(());
            },
            // Operation not completed, thus parse errno to find out what happened.
            Ok((None, size)) if size < 0 => {
                let errno: i32 = -size;
                // Operation in progress.
                if retry_errno(errno) {
                    if let Err(e) = yielder.yield_once().await {
                        let message: String = format!("pushto(): operation canceled (err={:?})", e);
                        error!("{}", message);
                        return Err(Fail::new(libc::ECANCELED, &message));
                    }
                } else {
                    let message: String = format!("push(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
            // Operation failed.
            Err(e) => {
                let message: String = format!("push(): operation failed (err={:?})", e);
                error!("{}", message);
                return Err(e);
            },
            // Should not happen.
            _ => panic!("push failed: unknown error"),
        }
    }
}
