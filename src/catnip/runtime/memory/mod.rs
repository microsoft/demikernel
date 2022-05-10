// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
pub mod consts;
mod dpdkbuf;
mod manager;
mod mbuf;
mod mempool;

//==============================================================================
// Exports
//==============================================================================

pub use dpdkbuf::DPDKBuf;
pub use manager::{
    Mbuf,
    MemoryManager,
};

//==============================================================================
// Imports
//==============================================================================

use super::DPDKRuntime;
use ::runtime::{
    fail::Fail,
    memory::MemoryRuntime,
    types::demi_sgarray_t,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for DPDK Runtime
impl MemoryRuntime for DPDKRuntime {
    type Buf = DPDKBuf;

    /// Casts a [DPDKBuf] into an [demi_sgarray_t].
    fn into_sgarray(&self, buf: Self::Buf) -> Result<demi_sgarray_t, Fail> {
        self.mm.into_sgarray(buf)
    }

    /// Allocates a [demi_sgarray_t].
    fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.mm.alloc_sgarray(size)
    }

    /// Releases a [demi_sgarray_t].
    fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.mm.free_sgarray(sga)
    }

    /// Clones a [demi_sgarray_t].
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<Self::Buf, Fail> {
        self.mm.clone_sgarray(sga)
    }
}
