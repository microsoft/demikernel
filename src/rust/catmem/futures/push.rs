// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::SharedRingBuffer,
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
pub async fn push_coroutine(ring: Rc<SharedRingBuffer<u16>>, buf: DemiBuffer, yielder: Yielder) -> Result<(), Fail> {
    let mut index: usize = 0;
    loop {
        for low in &buf[index..] {
            let x: u16 = (low & 0xff) as u16;
            match ring.try_enqueue(x) {
                Ok(()) => index += 1,
                Err(_) => {
                    // Operation not completed. Check if it was cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                },
            }
        }
        trace!("data written ({:?}/{:?} bytes)", index, buf.len());
        return Ok(());
    }
}
