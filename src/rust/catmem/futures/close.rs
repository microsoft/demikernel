// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::rc::Rc;

use crate::{
    catmem::CatmemRingBuffer,
    runtime::fail::Fail,
    scheduler::Yielder,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// End of file signal.
const EOF: u16 = (1 & 0xff) << 8;

/// Maximum number of retries for pushing a EoF signal.
const MAX_RETRIES_PUSH_EOF: u32 = 16;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// This function calls close on a file descriptor until it is closed successfully.
/// TODO merge this with push_eof(), when async_close() and close() are merged.
pub async fn close_coroutine(ring: Rc<CatmemRingBuffer>, yielder: Yielder) -> Result<(), Fail> {
    // Maximum number of retries. This is set to an arbitrary small value.
    let mut retries: u32 = MAX_RETRIES_PUSH_EOF;

    loop {
        match ring.try_enqueue(EOF) {
            // Operation completed.
            Ok(()) => break,
            // Operation not completed yet, check what happened.
            Err(_) => {
                retries -= 1;
                // Check if we have retried many times.
                if retries == 0 {
                    // We did, thus fail.
                    let cause: String = format!("failed to push EoF");
                    error!("push_eof(): {}", cause);
                    return Err(Fail::new(libc::EIO, &cause));
                } else {
                    // We did not, thus retry.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                }
            },
        }
    }

    Ok(())
}

/// Pushes the EoF signal to a shared ring buffer.
/// TODO merge this with close_coroutine(), when async_close() and close() are merged.
pub fn push_eof(ring: Rc<CatmemRingBuffer>) -> Result<(), Fail> {
    // Maximum number of retries. This is set to an arbitrary small value.
    let mut retries: u32 = MAX_RETRIES_PUSH_EOF;
    const EOF: u16 = (1 & 0xff) << 8;

    loop {
        match ring.try_enqueue(EOF) {
            Ok(()) => break,
            Err(_) => {
                retries -= 1;
                if retries == 0 {
                    let cause: String = format!("failed to push EoF");
                    error!("push_eof(): {}", cause);
                    return Err(Fail::new(libc::EIO, &cause));
                }
            },
        }
    }

    Ok(())
}
