// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::MAGIC_NUMBER;
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
    // Push a magic number.
    push_byte(&ring, MAGIC_NUMBER, &yielder).await?;
    // Push the length of the buffer.
    for byte in (buf.len() as u16).to_be_bytes() {
        push_byte(&ring, byte, &yielder).await?;
    }
    // Push the data.
    for byte in &buf[..] {
        push_byte(&ring, *byte, &yielder).await?;
    }
    trace!("data written ({:?}/{:?} bytes)", buf.len(), buf.len());
    return Ok(());
}

pub async fn push_byte(ring: &Rc<CatmemRingBuffer>, byte: u8, yielder: &Yielder) -> Result<(), Fail> {
    // Try to push the given byte.
    loop {
        match ring.try_enqueue(byte) {
            Ok(()) => return Ok(()),
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
