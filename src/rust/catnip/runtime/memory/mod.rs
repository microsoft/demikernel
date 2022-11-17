// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
pub mod consts;
mod manager;
mod mempool;

//==============================================================================
// Exports
//==============================================================================

pub use self::manager::MemoryManager;

//==============================================================================
// Imports
//==============================================================================

use super::DPDKRuntime;
use crate::runtime::{
    fail::Fail,
    memory::{
        DemiBuffer,
        MemoryRuntime,
    },
    types::demi_sgarray_t,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for DPDK Runtime
impl MemoryRuntime for DPDKRuntime {
    /// Casts a [DPDKBuf] into an [demi_sgarray_t].
    fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
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
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        self.mm.clone_sgarray(sga)
    }
}
