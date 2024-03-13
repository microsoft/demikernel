// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    alloc::LayoutError,
    num::NonZeroUsize,
    rc::Rc,
};

use crate::{
    pal::arch::CPU_DATA_CACHE_LINE_SIZE,
    runtime::memory::{
        demibuffer::MetaData,
        memory_pool::MemoryPool,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure is a wrapper around the [`MemoryPool`] concept to allow easier interoperation between that type and
/// [`DemiBuffer`]. This pool will create a buffer layout compatible with the metadata and requested user data size.
/// `DemiBuffer` can operate directly on this pool type.
pub struct BufferPool(Rc<MemoryPool>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl BufferPool {
    pub fn new(buffer_data_size: u16) -> Result<Self, LayoutError> {
        Ok(Self(MemoryPool::new(
            NonZeroUsize::new(std::mem::size_of::<MetaData>() + buffer_data_size as usize).unwrap(),
            NonZeroUsize::new(CPU_DATA_CACHE_LINE_SIZE).unwrap(),
        )?))
    }

    /// Get a reference to the underlying [`MemoryPool`].
    pub fn pool(&self) -> &Rc<MemoryPool> {
        &self.0
    }
}

// Unit tests for `BufferPool` type.
#[cfg(test)]
mod tests {
    use std::{
        mem::MaybeUninit,
        num::NonZeroUsize,
        ptr::NonNull,
    };

    use ::anyhow::Result;
    use anyhow::{
        anyhow,
        ensure,
    };

    use crate::{
        ensure_eq,
        runtime::memory::{
            BufferPool,
            DemiBuffer,
            MetaData,
        },
    };

    #[test]
    fn get_buffer_from_pool() -> Result<()> {
        const BUFFER_SIZE: usize = 0x1000;
        const PAGE_SIZE: usize = 0x80000000;
        let mut buffer: Vec<MaybeUninit<u8>> = Vec::with_capacity(BUFFER_SIZE);
        buffer.resize(buffer.capacity(), MaybeUninit::uninit());
        let pool: BufferPool = BufferPool::new(u16::try_from(buffer.len() - std::mem::size_of::<MetaData>())?)?;

        unsafe {
            pool.pool().populate(
                NonNull::from(buffer.as_mut_slice()),
                NonZeroUsize::new(PAGE_SIZE).unwrap(),
            )?
        };

        ensure_eq!(pool.pool().len(), 1);

        let buffer: DemiBuffer = DemiBuffer::new_in_pool(&pool).ok_or(anyhow!("could not create buffer"))?;

        ensure_eq!(buffer.len(), BUFFER_SIZE - std::mem::size_of::<MetaData>());
        ensure!(pool.pool().is_empty());

        std::mem::drop(buffer);
        ensure_eq!(pool.pool().len(), 1);

        ensure_eq!(pool.pool().get().unwrap().len(), BUFFER_SIZE);

        Ok(())
    }
}
