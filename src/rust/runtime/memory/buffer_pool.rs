// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    alloc::LayoutError,
    num::NonZeroUsize,
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
pub struct BufferPool(MemoryPool);

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
    pub fn pool(&self) -> &MemoryPool {
        &self.0
    }
}

// Unit tests for `BufferPool` type.
#[cfg(test)]
mod tests {
    use ::anyhow::Result;

    #[test]
    fn basic() -> Result<()> {
        Ok(())
    }
}
