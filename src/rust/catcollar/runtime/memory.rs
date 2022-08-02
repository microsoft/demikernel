// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::IoUringRuntime;
use ::libc::c_void;
use ::runtime::{
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
};
use ::std::{
    mem,
    ptr,
    slice,
    sync::Arc,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for I/O User Ring Runtime
impl MemoryRuntime for IoUringRuntime {
    /// Creates a scatter-gather array from a memory buffer.
    fn into_sgarray(&self, buf: Buffer) -> Result<demi_sgarray_t, Fail> {
        let len: usize = buf.len();
        let sgaseg: demi_sgaseg_t = match buf {
            Buffer::Heap(dbuf) => {
                let dbuf_ptr: *const [u8] = DataBuffer::into_raw(Clone::clone(&dbuf))?;
                demi_sgaseg_t {
                    sgaseg_buf: dbuf_ptr as *mut c_void,
                    sgaseg_len: len as u32,
                }
            },
        };
        Ok(demi_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Allocates a scatter-gather array.
    fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        let allocation: Arc<[u8]> = unsafe { Arc::new_uninit_slice(size).assume_init() };
        let ptr: *const [u8] = Arc::into_raw(allocation);
        let sgaseg = demi_sgaseg_t {
            sgaseg_buf: ptr as *mut _,
            sgaseg_len: size as u32,
        };
        Ok(demi_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Releases a scatter-gather array.
    fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        assert_eq!(sga.sga_numsegs, 1);
        for i in 0..sga.sga_numsegs as usize {
            let seg: &demi_sgaseg_t = &sga.sga_segs[i];
            let allocation: Arc<[u8]> = unsafe {
                Arc::from_raw(slice::from_raw_parts_mut(
                    seg.sgaseg_buf as *mut _,
                    seg.sgaseg_len as usize,
                ))
            };
            drop(allocation);
        }

        Ok(())
    }

    /// Clones a scatter-gather array into a memory buffer.
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<Buffer, Fail> {
        let mut len: u32 = 0;
        for i in 0..sga.sga_numsegs as usize {
            len += sga.sga_segs[i].sgaseg_len;
        }
        let mut buf: Buffer = Buffer::Heap(DataBuffer::new(len as usize).unwrap());
        let mut pos: usize = 0;
        for i in 0..sga.sga_numsegs as usize {
            let seg: &demi_sgaseg_t = &sga.sga_segs[i];
            let seg_slice: Arc<[u8]> = unsafe {
                Arc::from_raw(slice::from_raw_parts(
                    seg.sgaseg_buf as *mut u8,
                    seg.sgaseg_len as usize,
                ))
            };
            buf[pos..(pos + seg_slice.len())].copy_from_slice(&seg_slice[..]);
            pos += seg_slice.len();
        }
        Ok(buf)
    }
}
