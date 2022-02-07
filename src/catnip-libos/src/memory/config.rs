// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Structures
//==============================================================================

// TODO: Introduce Getters for field member of this structure.
#[derive(Clone, Copy, Debug)]
pub struct MemoryConfig {
    /// What is the cutoff point for copying application buffers into reserved body space within a
    /// header `mbuf`? Smaller values copy less but incur the fixed cost of chaining together
    /// `mbuf`s earlier.
    pub inline_body_size: usize,

    /// How many buffers are within the header pool?
    pub header_pool_size: usize,

    /// How many buffers are within the indirect pool?
    pub indirect_pool_size: usize,

    /// What is the maximum body size? This should effectively be the MSS + RTE_PKTMBUF_HEADROOM.
    pub max_body_size: usize,

    /// How many buffers are within the body pool?
    pub body_pool_size: usize,

    /// How many buffers should remain within `rte_mempool`'s per-thread cache?
    pub cache_size: usize,
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            inline_body_size: 1024,
            header_pool_size: 8191,
            indirect_pool_size: 8191,
            max_body_size: 8320,
            body_pool_size: 8191,
            cache_size: 250,
        }
    }
}
