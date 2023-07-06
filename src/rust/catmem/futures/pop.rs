// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    EOF,
    MAGIC_NUMBER,
};
use crate::{
    catmem::CatmemRingBuffer,
    runtime::{
        fail::Fail,
        limits,
        memory::DemiBuffer,
    },
    scheduler::yielder::Yielder,
};
use ::std::rc::Rc;

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Polls `try_dequeue()` on `ring` until some data is received and placed in `buf`.
pub async fn pop_coroutine(
    ring: Rc<CatmemRingBuffer>,
    size: Option<usize>,
    yielder: Yielder,
) -> Result<(DemiBuffer, bool), Fail> {
    let size: usize = size.unwrap_or(limits::RECVBUF_SIZE_MAX);
    let mut buf: DemiBuffer = DemiBuffer::new(size as u16);
    let mut eof: bool = false;

    // Pop magic number
    let header: u8 = pop_byte(&ring, &yielder).await?;
    if header == MAGIC_NUMBER {
        let len_bytes: [u8; 2] = [pop_byte(&ring, &yielder).await?, pop_byte(&ring, &yielder).await?];
        let len: usize = u16::from_be_bytes(len_bytes) as usize;

        if len <= size {
            buf.trim(size - len)?;
            for index in 0..len {
                buf[index] = pop_byte(&ring, &yielder).await?;
            }
        } else {
            // FIXME: if this happens we need to track how much we read, so we will need to pass in the pipe data //
            // structure so we have some place to store metadata.
        }
    } else if header == EOF {
        buf.trim(size)?;
        eof = true;
    } else {
        // panic?
        panic!(
            "Incorrect message header: Found {:?}, expected MAGIC={:?} or EOF={:?}",
            header, MAGIC_NUMBER, EOF
        )
    };
    trace!("data read ({:?}/{:?} bytes, eof={:?})", buf.len(), size, eof);
    Ok((buf, eof))
}

pub async fn pop_byte(ring: &Rc<CatmemRingBuffer>, yielder: &Yielder) -> Result<u8, Fail> {
    // Try to pop the given byte
    loop {
        match ring.try_dequeue() {
            Some(x) => break Ok(x),
            None => {
                // Operation in progress. Check if cancelled.
                match yielder.yield_once().await {
                    Ok(()) => continue,
                    Err(cause) => break Err(cause),
                }
            },
        }
    }
}
