// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::DPDKRuntime;
use crate::catnip::memory::DPDKBuf;
use ::runtime::{
    memory::MemoryRuntime,
    types::dmtr_sgarray_t,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for DPDK Runtime
impl MemoryRuntime for DPDKRuntime {
    type Buf = DPDKBuf;

    fn into_sgarray(&self, buf: Self::Buf) -> dmtr_sgarray_t {
        self.inner.borrow().memory_manager.into_sgarray(buf)
    }

    fn alloc_sgarray(&self, size: usize) -> dmtr_sgarray_t {
        self.inner.borrow().memory_manager.alloc_sgarray(size)
    }

    fn free_sgarray(&self, sga: dmtr_sgarray_t) {
        self.inner.borrow().memory_manager.free_sgarray(sga)
    }

    fn clone_sgarray(&self, sga: &dmtr_sgarray_t) -> Self::Buf {
        self.inner.borrow().memory_manager.clone_sgarray(sga)
    }
}
