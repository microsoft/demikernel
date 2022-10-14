// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    runtime::{
        fail::Fail,
        memory::{
            Buffer,
            DataBuffer,
            MemoryRuntime,
        },
        types::{
            demi_sgarray_t,
            demi_sgaseg_t,
        },
        Runtime,
    },
    scheduler::scheduler::Scheduler,
};
use ::libc::c_void;
use ::std::{
    mem,
    slice,
};

//==============================================================================
// Structures
//==============================================================================

/// POSIX Runtime
#[derive(Clone)]
pub struct PosixRuntime {
    /// Scheduler
    pub scheduler: Scheduler,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for POSIX Runtime
impl PosixRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for PosixRuntime {
    /// Converts a runtime buffer into a scatter-gather array.
    fn into_sgarray(&self, buf: Buffer) -> Result<demi_sgarray_t, Fail> {
        let len: usize = buf.len();
        #[allow(unreachable_patterns)]
        let (dbuf_ptr, sgaseg): (*const u8, demi_sgaseg_t) = match buf {
            Buffer::Heap(dbuf) => {
                let (dbuf_ptr, data_ptr): (*const u8, *const u8) = DataBuffer::into_raw_parts(Clone::clone(&dbuf))?;
                (
                    dbuf_ptr,
                    demi_sgaseg_t {
                        sgaseg_buf: data_ptr as *mut c_void,
                        sgaseg_len: len as u32,
                    },
                )
            },
            _ => return Err(Fail::new(libc::EINVAL, "invalid buffer type")),
        };
        Ok(demi_sgarray_t {
            sga_buf: dbuf_ptr as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Allocates a scatter-gather array.
    fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // Allocate a heap-managed buffer.
        let dbuf: DataBuffer = DataBuffer::new(size)?;
        let (dbuf_ptr, data_ptr): (*const u8, *const u8) = DataBuffer::into_raw_parts(dbuf)?;
        let sgaseg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data_ptr as *mut c_void,
            sgaseg_len: size as u32,
        };
        Ok(demi_sgarray_t {
            sga_buf: dbuf_ptr as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Releases a scatter-gather array.
    fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "scatter-gather array with invalid size"));
        }

        // Release heap-managed buffer.
        let (dbuf_ptr, length): (*mut u8, usize) = (sga.sga_buf as *mut u8, sga.sga_segs[0].sgaseg_len as usize);
        DataBuffer::from_raw_parts(dbuf_ptr, length)?;

        Ok(())
    }

    /// Clones a scatter-gather array.
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<Buffer, Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "scatter-gather array with invalid size"));
        }

        let sgaseg: demi_sgaseg_t = sga.sga_segs[0];
        let (dbuf_ptr, len): (*mut c_void, usize) = (sga.sga_buf, sgaseg.sgaseg_len as usize);

        // Clone heap-managed buffer.
        let seg_slice: &[u8] = unsafe { slice::from_raw_parts(dbuf_ptr as *const u8, len) };
        let mut dbuf: DataBuffer = DataBuffer::from_slice(seg_slice);
        let nbytes: usize = unsafe { sgaseg.sgaseg_buf.sub_ptr(sga.sga_buf) };
        dbuf.adjust(nbytes);
        Ok(Buffer::Heap(dbuf))
    }
}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for PosixRuntime {}
