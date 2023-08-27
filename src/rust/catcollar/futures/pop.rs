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

pub async fn pop_coroutine(
    mut rt: IoUringRuntime,
    fd: RawFd,
    buf: DemiBuffer,
    yielder: Yielder,
) -> Result<(Option<SocketAddrV4>, DemiBuffer), Fail> {
    let request_id: RequestId = rt.pop(fd, buf.clone())?;
    loop {
        match rt.peek(request_id) {
            // Operation completed.
            Ok((addr, size)) if size >= 0 => {
                trace!("data received ({:?} bytes)", size);
                let trim_size: usize = buf.len() - (size as usize);
                let mut buf: DemiBuffer = buf.clone();
                buf.trim(trim_size)?;
                return Ok((addr, buf));
            },
            // Operation not completed, thus parse errno to find out what happened.
            Ok((None, size)) if size < 0 => {
                let errno: i32 = -size;
                if retry_errno(errno) {
                    if let Err(e) = yielder.yield_once().await {
                        let message: String = format!("pop(): operation canceled (err={:?})", e);
                        error!("{}", message);
                        return Err(Fail::new(libc::ECANCELED, &message));
                    }
                } else {
                    let message: String = format!("pop(): operation failed (errno={:?})", errno);
                    error!("{}", message);
                    return Err(Fail::new(errno, &message));
                }
            },
            // Operation failed.
            Err(e) => {
                let message: String = format!("pop(): operation failed (err={:?})", e);
                error!("{}", message);
                return Err(e);
            },
            // Should not happen.
            _ => panic!("pop failed: unknown error"),
        }
    }
}
