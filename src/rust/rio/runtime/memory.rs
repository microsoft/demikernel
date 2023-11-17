// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::collections::VecDeque;

use crate::{
    demi_sgarray_t,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
    },
};

use super::RioRuntime;

//==============================================================================
// Structures
//==============================================================================

struct MemoryConfig {
    pool_send_recv: bool,
    send_buf_size: usize,
    recv_buf_size: usize,
}

pub struct BufferRing {
    buffer_size: usize,
    bufs: VecDeque<DemiBuffer>,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl BufferRing {
    pub fn new(ring_size: usize, buffer_size: usize) -> Result<Self, Fail> {
        let buf_size_u16: u16 =
            u16::try_from(buffer_size).map_err(|_| Fail::new(libc::ERANGE, "buffer_size too large"))?;

        let mut result = Self {
            buffer_size: buffer_size,
            bufs: VecDeque::with_capacity(ring_size),
        };

        for _ in 0..ring_size {
            result.bufs.push_back(DemiBuffer::new(buf_size_u16));
        }

        Ok(result)
    }

    /// Dequeue or create a scatter-gather array.
    pub fn sgaalloc(&mut self, mr: &RioRuntime, size: usize) -> Result<demi_sgarray_t, Fail> {
        trace!("sgalloc() size={:?}", size);
        if size <= self.buffer_size {
            if let Some(mut buf) = self.bufs.pop_front() {
                // Validate that enqueued buffers must have their size restored.
                debug_assert!(buf.len() == self.buffer_size);
                buf.trim(self.buffer_size - size)
                    .expect("invariant violation: non-homogeneous buffer size");
                return mr.into_sgarray(buf);
            }
        }

        mr.alloc_sgarray(std::cmp::max(size, self.buffer_size))
    }

    /// Requeue or free a scatter-gather array.
    pub fn sgafree(&mut self, mr: &RioRuntime, sga: demi_sgarray_t) -> Result<(), Fail> {
        trace!("sgafree()");
        let mut buf: DemiBuffer = mr.from_sgarray(sga)?;
        if buf.len() + buf.tailroom() == self.buffer_size {
            buf.append(buf.tailroom()).unwrap();
            self.bufs.push_back(buf);
        } else {
            std::mem::drop(buf);
        }

        Ok(())
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================
