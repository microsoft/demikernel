// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::CatmemRingBuffer,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
    scheduler::Yielder,
};
use ::std::rc::Rc;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Polls `try_enqueue()` on `ring` until all the data in the `buf` is sent.
pub async fn push_coroutine(ring: Rc<CatmemRingBuffer>, buf: DemiBuffer, yielder: Yielder) -> Result<(), Fail> {
    for low in &buf[..] {
        let x: u16 = (low & 0xff) as u16;
        loop {
            match ring.try_enqueue(x) {
                Ok(()) => break,
                Err(_) => {
                    // Operation not completed. Check if it was cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                },
            }
        }
    }
    trace!("data written ({:?}/{:?} bytes)", buf.len(), buf.len());
    return Ok(());
}
