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

/// DPDK-Managed Buffer
#[derive(Debug)]
pub struct Mbuf {
    /// Underlying DPDK buffer.
    ptr: *mut rte_mbuf,
    /// TODO: drop the following field.
    mm: MemoryManager,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for DPDK-Managed Buffers
impl Mbuf {
    /// Creates a [Mbuf].
    pub fn new(ptr: *mut rte_mbuf, mm: MemoryManager) -> Self {
        Mbuf { ptr, mm }
    }

    /// Removes `len` bytes at the beginning of the target [Mbuf].
    pub fn adjust(&mut self, len: usize) {
        assert!(len <= self.len());
        if unsafe { rte_pktmbuf_adj(self.ptr, len as u16) } == ptr::null_mut() {
            panic!("rte_pktmbuf_adj failed");
        }
    }

    /// Removes `len` bytes at the end of the target [Mbuf].
    pub fn trim(&mut self, len: usize) {
        assert!(len <= self.len());
        if unsafe { rte_pktmbuf_trim(self.ptr, len as u16) } != 0 {
            panic!("rte_pktmbuf_trim failed");
        }
    }

    /// Returns a pointer to the data stored in the target [Mbuf].
    pub fn data_ptr(&self) -> *mut u8 {
        unsafe {
            let buf_ptr = (*self.ptr).buf_addr as *mut u8;
            buf_ptr.offset((*self.ptr).data_off as isize)
        }
    }

    /// Returns the length of the data stored in the target [Mbuf].
    pub fn len(&self) -> usize {
        unsafe { (*self.ptr).data_len as usize }
    }

    /// Converts the target [Mbuf] into a mutable [u8] slice.
    pub unsafe fn slice_mut(&mut self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.data_ptr(), self.len())
    }

    /// Converts the target [Mbuf] into a raw DPDK buffer.
    pub fn into_raw(mut self) -> *mut rte_mbuf {
        mem::replace(&mut self.ptr, ptr::null_mut())
    }

    /// Returns a pointer to the underlying DPDK buffer stored in the target [Mbuf].
    pub fn get_ptr(&self) -> *mut rte_mbuf {
        self.ptr
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Clone Trait Implementation for DPDK-Managed Buffers
impl Clone for Mbuf {
    fn clone(&self) -> Self {
        self.mm.clone_mbuf(self)
    }
}

/// De-Reference Trait Implementation for DPDK-Managed Buffers
impl Deref for Mbuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data_ptr(), self.len()) }
    }
}

/// Drop Trait Implementation for DPDK-Managed Buffers
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
