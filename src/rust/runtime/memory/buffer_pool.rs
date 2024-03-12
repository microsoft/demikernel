// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    alloc::{
        Layout,
        LayoutError,
    },
    iter::{
        FromFn,
        Zip,
    },
    mem::{
        size_of,
        MaybeUninit,
    },
    num::{
        NonZeroU16,
        NonZeroUsize,
    },
    ptr::NonNull,
};

use crate::{
    pal::arch,
    runtime::{
        fail::Fail,
        memory::demibuffer::DemiBuffer,
    },
};

use super::MetaData;

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure provides a facility to preallocate DemiBuffers, then use the preallocated buffers on the data path.
/// Buffers are exposed via the [PooledBuffer] trait; this struct then interfaces with [DemiBuffer] to provide a safe
/// instance of the buffer, which is automatically returned to the ring when the DemiBuffer is dropped. The pool is
/// prepopulated with PooledBuffers and optionally configured with a factory which can construct additional buffers
/// if the pool is exhausted.
pub struct BufferPool {
    buffers: Vec<RawBuffer>,

    buffer_data_size: NonZeroUsize,
}

/// Raw parts of a DemiBuffer.
struct RawBuffer {
    /// Buffer for metadata + direct data
    metadata_buf: NonNull<[MaybeUninit<u8>]>,

    /// Buffer for external data
    data: Option<NonNull<[MaybeUninit<u8>]>>,
}

/// This struct tracks consumption of a buffer, allowing the caller to take subspans and skip bytes, with useful
/// pointer-alignment methods.
struct BufferCursor<'a>(&'a mut [MaybeUninit<u8>]);

/// An iterator which will pack objects of a specific layout into a series of memory pages. The algorithm will try to
/// minimize the number of pages each object spans, which may result in unused bytes at the end of each page. When the
/// aligned size is a factor or multiple of the page size, objects will be tightly packed.
struct PackingIterator<'a> {
    cursor: BufferCursor<'a>,
    layout: Layout,
    page_size: NonZeroUsize,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl BufferPool {}

impl RawBuffer {
    pub fn new_internal(buffer: &mut [MaybeUninit<u8>]) -> Self {
        debug_assert!(
            buffer.len() >= size_of::<MetaData>() && buffer.len() <= (u16::MAX as usize) + size_of::<MetaData>()
        );

        Self {
            metadata_buf: NonNull::from(buffer),
            data: None,
        }
    }

    pub fn new_external(metadata: &mut [MaybeUninit<u8>], data: &mut [MaybeUninit<u8>]) -> Self {
        debug_assert!(metadata.len() >= size_of::<MetaData>() && data.len() <= u16::MAX as usize);

        Self {
            metadata_buf: NonNull::from(metadata),
            data: Some(NonNull::from(data)),
        }
    }
}

impl<'a> BufferCursor<'a> {
    pub fn new(buffer: &'a mut [MaybeUninit<u8>]) -> Self {
        Self(buffer)
    }
}

impl<'a> PackingIterator<'a> {}

// Unit tests for `BufferPool` type.
#[cfg(test)]
mod tests {
    use ::anyhow::Result;

    #[test]
    fn basic() -> Result<()> {
        Ok(())
    }
}
