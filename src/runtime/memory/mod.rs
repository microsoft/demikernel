// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod buffer_pool;
mod demibuffer;
mod memory_pool;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    types::{demi_sgarray_t, demi_sgaseg_t, DEMI_SGARRAY_MAXLEN},
};
use ::arrayvec::ArrayVec;
use ::libc::c_void;
use ::std::{mem, ptr::NonNull};

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{buffer_pool::*, demibuffer::*};

//======================================================================================================================
// Traits
//======================================================================================================================

pub trait DemiMemoryAllocator {
    fn max_buffer_size_bytes(&self) -> usize {
        u16::MAX as usize
    }

    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        Ok(DemiBuffer::new(size as u16))
    }
}

pub fn into_sgarray(buffers: ArrayVec<DemiBuffer, DEMI_SGARRAY_MAXLEN>) -> Result<demi_sgarray_t, Fail> {
    if buffers.is_empty() {
        error!("into_sgarray(): buffers is empty");
        return Err(Fail::new(libc::EINVAL, "buffers is empty"));
    }

    if buffers.len() > DEMI_SGARRAY_MAXLEN {
        error!(
            "into_sgarray(): too many buffers: {}, max: {}",
            buffers.len(),
            DEMI_SGARRAY_MAXLEN
        );
        return Err(Fail::new(libc::EINVAL, "too many buffers"));
    }

    let mut sga: demi_sgarray_t = demi_sgarray_t {
        num_segments: buffers.len() as u32,
        ..Default::default()
    };

    for (i, buffer) in buffers.into_iter().enumerate() {
        sga.segments[i].data_buf_ptr = buffer.as_ptr() as *mut c_void;
        sga.segments[i].data_len_bytes = buffer.len() as u32;
        sga.segments[i].reserved_metadata_ptr = buffer.into_raw().as_ptr() as *mut c_void;
    }

    Ok(sga)
}

pub fn sgaalloc<M: DemiMemoryAllocator>(size: usize, mem_alloc: &M) -> Result<demi_sgarray_t, Fail> {
    if size == 0 {
        error!("sgaalloc(): cannot allocate zero-sized buffer");
        return Err(Fail::new(libc::EINVAL, "cannot allocate zero-sized buffer"));
    }

    // First allocate the underlying DemiBuffer.
    if size > mem_alloc.max_buffer_size_bytes() * DEMI_SGARRAY_MAXLEN {
        error!(
            "sgaalloc(): size too large for a single demi_sgaseg_t: {}, max: {}",
            size,
            mem_alloc.max_buffer_size_bytes() * DEMI_SGARRAY_MAXLEN
        );
        return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
    }

    // Calculate the number of DemiBuffers to allocate.
    let max_buffer_size_bytes: usize = mem_alloc.max_buffer_size_bytes();
    let remainder: usize = size % max_buffer_size_bytes;
    let len: usize = (size - remainder) / max_buffer_size_bytes;
    let mut bufs: ArrayVec<DemiBuffer, DEMI_SGARRAY_MAXLEN> = ArrayVec::new();
    trace!(
        "sgaalloc(): max_buffer_size_bytes {}, len {}, remainder {}",
        max_buffer_size_bytes,
        len,
        remainder
    );

    for _ in 0..len {
        bufs.push(mem_alloc.allocate_demi_buffer(max_buffer_size_bytes)?);
    }

    // If there is any remaining length, allocate a partial buffer.
    if remainder > 0 {
        bufs.push(mem_alloc.allocate_demi_buffer(remainder)?);
    }

    into_sgarray(bufs)
}

pub fn sgafree(sga: demi_sgarray_t) -> Result<(), Fail> {
    if sga.num_segments > DEMI_SGARRAY_MAXLEN as u32 {
        return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid segment count"));
    }

    for i in 0..sga.num_segments as usize {
        let buf: DemiBuffer = convert_sgaseg_to_demi_buffer(&sga.segments[i])?;
        drop(buf);
    }

    Ok(())
}

/// The [sga_buf] field must point to the first DemiBuffer in the chain and the elements of [segments] must be the rest
/// of the chain.
pub fn clone_sgarray(sga: &demi_sgarray_t) -> Result<ArrayVec<DemiBuffer, DEMI_SGARRAY_MAXLEN>, Fail> {
    if sga.num_segments > DEMI_SGARRAY_MAXLEN as u32 || sga.num_segments == 0 {
        return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid segment count"));
    }

    let mut bufs: ArrayVec<DemiBuffer, DEMI_SGARRAY_MAXLEN> = ArrayVec::new();
    for i in 0..sga.num_segments as usize {
        // Convert back to a DemiBuffer.
        let buf: DemiBuffer = convert_sgaseg_to_demi_buffer(&sga.segments[i])?;
        // Clone the DemiBuffer, this will recursively clone the entire chain.
        let clone: DemiBuffer = buf.clone();

        // Don't drop buf, as it holds the same reference to the data as the sgarray (which should keep it).
        mem::forget(buf);
        bufs.push(clone);
    }
    Ok(bufs)
}

fn convert_sgaseg_to_demi_buffer(sga_seg: &demi_sgaseg_t) -> Result<DemiBuffer, Fail> {
    if sga_seg.reserved_metadata_ptr.is_null() {
        return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid DemiBuffer token"));
    }
    // Convert back to a DemiBuffer and drop it.
    // Safety: The `NonNull::new_unchecked()` call is safe, as we verified `sga.sga_buf` is not null above.
    let token: NonNull<u8> = unsafe { NonNull::new_unchecked(sga_seg.reserved_metadata_ptr as *mut u8) };
    // Safety: The `DemiBuffer::from_raw()` call *should* be safe, as the `sga_buf` field in the `demi_sgarray_t`
    // contained a valid `DemiBuffer` token when we provided it to the user (and the user shouldn't change it).
    let mut buf: DemiBuffer = unsafe { DemiBuffer::from_raw(token) };
    check_demi_buf_limits(sga_seg, &mut buf)?;
    Ok(buf)
}

/// Check to see if the user has reduced the size of the buffer described by the sgarray segment since we provided it to
/// them.  They could have increased the starting address of the buffer (`data_buf_ptr`), decreased the ending address of
/// the buffer (`data_buf_ptr + data_buf_len`), or both.
fn check_demi_buf_limits(sga_seg: &demi_sgaseg_t, clone: &mut DemiBuffer) -> Result<(), Fail> {
    let sga_data: *const u8 = sga_seg.data_buf_ptr as *const u8;
    let sga_len: usize = sga_seg.data_len_bytes as usize;
    let clone_data: *const u8 = clone.as_ptr();
    let mut clone_len: usize = clone.len();
    if sga_data != clone_data || sga_len != clone_len {
        // We need to adjust the DemiBuffer to match the user's changes.

        // First check that the user didn't do something non-sensical, like change the buffer description to
        // reference address space outside of the DemiBuffer's allocated memory area.
        if sga_data < clone_data || sga_data.addr() + sga_len > clone_data.addr() + clone_len {
            return Err(Fail::new(
                libc::EINVAL,
                "demi_sgarray_t describes data outside backing buffer's allocated region",
            ));
        }

        // Calculate the amount the new starting address is ahead of the old.  And then adjust `clone` to match.
        let adjustment_amount = sga_data.addr() - clone_data.addr();
        clone.adjust(adjustment_amount)?;

        // An adjustment above would have reduced clone.len() by the adjustment amount.
        clone_len -= adjustment_amount;
        debug_assert_eq!(clone_len, clone.len());

        // Trim the clone down to size.
        let trim_amount = clone_len - sga_len;
        clone.trim(trim_amount)?;
    }
    Ok(())
}
