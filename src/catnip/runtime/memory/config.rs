// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::consts::{
    DEFAULT_BODY_POOL_SIZE,
    DEFAULT_CACHE_SIZE,
    DEFAULT_HEADER_POOL_SIZE,
    DEFAULT_INDIRECT_POOL_SIZE,
    DEFAULT_INLINE_BODY_SIZE,
    DEFAULT_MAX_BODY_SIZE,
};

//==============================================================================
// Structures
//==============================================================================

//// Memory Configuration Descriptor
///
/// TODO: Introduce Setters for field member of this structure.
#[derive(Debug)]
pub struct MemoryConfig {
    /// What is the cutoff point for copying application buffers into reserved body space within a
    /// header `mbuf`? Smaller values copy less but incur the fixed cost of chaining together
    /// `mbuf`s earlier.
    inline_body_size: usize,

    /// How many buffers are within the header pool?
    header_pool_size: usize,

    /// How many buffers are within the indirect pool?
    indirect_pool_size: usize,

    /// What is the maximum body size? This should effectively be the MSS + RTE_PKTMBUF_HEADROOM.
    max_body_size: usize,

    /// How many buffers are within the body pool?
    body_pool_size: usize,

    /// How many buffers should remain within `rte_mempool`'s per-thread cache?
    cache_size: usize,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Memory Configuration Descriptors
impl MemoryConfig {
    pub fn new(
        inline_body_size: Option<usize>,
        header_pool_size: Option<usize>,
        indirect_pool_size: Option<usize>,
        max_body_size: Option<usize>,
        body_pool_size: Option<usize>,
        cache_size: Option<usize>,
    ) -> Self {
        let mut config: Self = Self::default();

        // Sets the inline body size config option.
        if let Some(inline_body_size) = inline_body_size {
            config.inline_body_size = inline_body_size;
        }

        // Sets the header pool size config option.
        if let Some(header_pool_size) = header_pool_size {
            config.header_pool_size = header_pool_size;
        }

        // Sets the indirect pool size config option.
        if let Some(indirect_pool_size) = indirect_pool_size {
            config.indirect_pool_size = indirect_pool_size;
        }

        // Sets the max body pool size config option.
        if let Some(max_body_size) = max_body_size {
            config.max_body_size = max_body_size;
        }

        // Sets the body pool size config option.
        if let Some(body_pool_size) = body_pool_size {
            config.body_pool_size = body_pool_size;
        }

        // Sets the cache size config option.
        if let Some(cache_size) = cache_size {
            config.cache_size = cache_size;
        }

        config
    }

    /// Returns the inline body size config stored in the target [MemoryConfig].
    pub fn get_inline_body_size(&self) -> usize {
        self.inline_body_size
    }

    /// Returns the header pool size config stored in the target [MemoryConfig].
    pub fn get_header_pool_size(&self) -> usize {
        self.header_pool_size
    }

    /// Returns the indirect pool size config stored in the target [MemoryConfig].
    pub fn get_indirect_pool_size(&self) -> usize {
        self.indirect_pool_size
    }

    /// Returns the max body size config stored in the target [MemoryConfig].
    pub fn get_max_body_size(&self) -> usize {
        self.max_body_size
    }

    /// Returns the body pool size config stored in the target [MemoryConfig].
    pub fn get_body_pool_size(&self) -> usize {
        self.body_pool_size
    }

    /// Returns the cache size config stored in the target [MemoryConfig].
    pub fn get_cache_size(&self) -> usize {
        self.cache_size
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Default Trait Implementation for Memory Configuration Descriptors
impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            inline_body_size: DEFAULT_INLINE_BODY_SIZE,
            header_pool_size: DEFAULT_HEADER_POOL_SIZE,
            indirect_pool_size: DEFAULT_INDIRECT_POOL_SIZE,
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            body_pool_size: DEFAULT_BODY_POOL_SIZE,
            cache_size: DEFAULT_CACHE_SIZE,
        }
    }
}
