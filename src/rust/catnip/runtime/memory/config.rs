// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::catnip::runtime::memory::consts::{
    DEFAULT_BODY_POOL_SIZE,
    DEFAULT_CACHE_SIZE,
    DEFAULT_MAX_BODY_SIZE,
};

//==============================================================================
// Structures
//==============================================================================

//// Memory Configuration Descriptor
#[derive(Debug)]
pub struct MemoryConfig {
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
    pub fn new(max_body_size: Option<usize>, body_pool_size: Option<usize>, cache_size: Option<usize>) -> Self {
        let mut config: Self = Self::default();

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
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            body_pool_size: DEFAULT_BODY_POOL_SIZE,
            cache_size: DEFAULT_CACHE_SIZE,
        }
    }
}
