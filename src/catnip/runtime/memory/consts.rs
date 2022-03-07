// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::dpdk_rs::{
    RTE_MBUF_DEFAULT_BUF_SIZE,
    RTE_PKTMBUF_HEADROOM,
};

//==============================================================================
// Constants
//==============================================================================

/// Default size for inline body buffers.
pub const DEFAULT_INLINE_BODY_SIZE: usize = 1024;

/// Default number of buffers in the header pool.
pub const DEFAULT_HEADER_POOL_SIZE: usize = 8192 - 1;

/// Default number of buffers in the indirect pool.
pub const DEFAULT_INDIRECT_POOL_SIZE: usize = 8192 - 1;

/// Default number of buffers in the body pool.
pub const DEFAULT_BODY_POOL_SIZE: usize = 8192 - 1;

/// Default value for maximum body size.
pub const DEFAULT_MAX_BODY_SIZE: usize =
    (RTE_MBUF_DEFAULT_BUF_SIZE + RTE_PKTMBUF_HEADROOM) as usize;

/// Default per-thread cache size.
pub const DEFAULT_CACHE_SIZE: usize = 250;
