// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

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
    let mut index: usize = 0;
    loop {
        match ring.try_dequeue() {
            Some(x) => {
                let (high, low): (u8, u8) = (((x >> 8) & 0xff) as u8, (x & 0xff) as u8);
                if high != 0 {
                    buf.trim(size - index)
                        .expect("cannot trim more bytes than the buffer has");
                    eof = true;
                    break;
                } else {
                    buf[index] = low;
                    index += 1;

                    // Check if we read enough bytes.
                    if index >= size {
                        buf.trim(size - index)
                            .expect("cannot trim more bytes than the buffer has");
                        break;
                    }
                }
            },
            None => {
                if index > 0 {
                    buf.trim(size - index)
                        .expect("cannot trim more bytes than the buffer has");
                    break;
                } else {
                    // Operation in progress. Check if cancelled.
                    match yielder.yield_once().await {
                        Ok(()) => continue,
                        Err(cause) => return Err(cause),
                    }
                }
            },
        }
    }
    trace!("data read ({:?}/{:?} bytes, eof={:?})", buf.len(), size, eof);
    Ok((buf, eof))
}
