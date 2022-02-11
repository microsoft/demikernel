// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::MemoryManager;
use ::dpdk_rs::{
    rte_mbuf,
    rte_pktmbuf_adj,
    rte_pktmbuf_free,
    rte_pktmbuf_trim,
};
use ::std::{
    mem,
    ops::Deref,
    ptr,
    slice,
};

//==============================================================================
// Structures
//==============================================================================

#[derive(Debug)]
pub struct Mbuf {
    pub ptr: *mut rte_mbuf,
    pub mm: MemoryManager,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl Mbuf {
    /// Remove `len` bytes at the beginning of the mbuf.
    pub fn adjust(&mut self, len: usize) {
        assert!(len <= self.len());
        if unsafe { rte_pktmbuf_adj(self.ptr, len as u16) } == ptr::null_mut() {
            panic!("rte_pktmbuf_adj failed");
        }
    }

    /// Remove `len` bytes at the end of the mbuf.
    pub fn trim(&mut self, len: usize) {
        assert!(len <= self.len());
        if unsafe { rte_pktmbuf_trim(self.ptr, len as u16) } != 0 {
            panic!("rte_pktmbuf_trim failed");
        }
    }

    pub fn split(self, ix: usize) -> (Self, Self) {
        let n = self.len();
        if ix == n {
            let empty = Self {
                ptr: self.mm.inner.alloc_indirect_empty(),
                mm: self.mm.clone(),
            };
            return (self, empty);
        }

        let mut suffix = self.clone();
        let mut prefix = self;

        prefix.trim(n - ix);
        suffix.adjust(ix);
        (prefix, suffix)
    }

    pub fn data_ptr(&self) -> *mut u8 {
        unsafe {
            let buf_ptr = (*self.ptr).buf_addr as *mut u8;
            buf_ptr.offset((*self.ptr).data_off as isize)
        }
    }

    pub fn len(&self) -> usize {
        unsafe { (*self.ptr).data_len as usize }
    }

    pub unsafe fn slice_mut(&mut self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.data_ptr(), self.len())
    }

    pub fn into_raw(mut self) -> *mut rte_mbuf {
        mem::replace(&mut self.ptr, ptr::null_mut())
    }

    pub fn ptr(&mut self) -> *mut rte_mbuf {
        self.ptr
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Clone for Mbuf {
    fn clone(&self) -> Self {
        self.mm.clone_mbuf(self)
    }
}

impl Deref for Mbuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data_ptr(), self.len()) }
    }
}

impl Drop for Mbuf {
    fn drop(&mut self) {
        if self.ptr.is_null() {
            return;
        }
        unsafe {
            rte_pktmbuf_free(self.ptr);
        }
        self.ptr = ptr::null_mut();
    }
}
