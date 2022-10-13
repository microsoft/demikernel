// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::fail::Fail;
use ::dpdk_rs::{
    rte_mbuf,
    rte_mempool,
    rte_pktmbuf_adj,
    rte_pktmbuf_clone,
    rte_pktmbuf_free,
    rte_pktmbuf_trim,
};
use ::std::{
    mem,
    ops::{
        Deref,
        DerefMut,
    },
    ptr,
    slice,
};

//==============================================================================
// Structures
//==============================================================================

/// DPDK-Managed Buffer
#[derive(Debug)]
pub struct DPDKBuffer {
    /// Underlying DPDK buffer.
    ptr: *mut rte_mbuf,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for DPDK-Managed Buffers
impl DPDKBuffer {
    /// Removes `len` bytes at the beginning of the target [Mbuf].
    pub fn adjust(&mut self, nbytes: usize) {
        assert!(nbytes <= self.len(), "nbytes={:?} self.len()={:?}", nbytes, self.len());
        if unsafe { rte_pktmbuf_adj(self.ptr, nbytes as u16) } == ptr::null_mut() {
            panic!("rte_pktmbuf_adj failed");
        }
    }

    /// Removes `len` bytes at the end of the target [Mbuf].
    pub fn trim(&mut self, nbytes: usize) {
        assert!(nbytes <= self.len(), "nbytes={:?} self.len()={:?}", nbytes, self.len());
        assert!(nbytes <= self.len());
        if unsafe { rte_pktmbuf_trim(self.ptr, nbytes as u16) } != 0 {
            panic!("rte_pktmbuf_trim failed");
        }
    }

    /// Creates a [Mbuf].
    pub fn new(ptr: *mut rte_mbuf) -> Self {
        DPDKBuffer { ptr }
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

/// De-Reference Trait Implementation for DPDK-Managed Buffers
impl Deref for DPDKBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data_ptr(), self.len()) }
    }
}

/// Mutable De-Reference Trait Implementation for DPDK-Managed Buffers
impl DerefMut for DPDKBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.data_ptr(), self.len()) }
    }
}

/// Clone Trait Implementation for DPDK-Managed Buffers
impl Clone for DPDKBuffer {
    fn clone(&self) -> Self {
        let mbuf_ptr: *mut rte_mbuf = match clone_mbuf(self.ptr) {
            Ok(mbuf_ptr) => mbuf_ptr,
            Err(e) => panic!("failed to clone mbuf: {:?}", e.cause),
        };

        DPDKBuffer::new(mbuf_ptr)
    }
}

/// Drop Trait Implementation for DPDK-Managed Buffers
impl Drop for DPDKBuffer {
    fn drop(&mut self) {
        if self.ptr.is_null() {
            return;
        }
        free_mbuf(self.ptr);
        self.ptr = ptr::null_mut();
    }
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Releases a mbuf in the target memory pool.
fn free_mbuf(mbuf_ptr: *mut rte_mbuf) {
    unsafe {
        rte_pktmbuf_free(mbuf_ptr);
    }
}

/// Clones a mbuf into a memory pool.
fn clone_mbuf(mbuf_ptr: *mut rte_mbuf) -> Result<*mut rte_mbuf, Fail> {
    unsafe {
        let mempool_ptr: *mut rte_mempool = (*mbuf_ptr).pool;
        let mbuf_ptr_clone: *mut rte_mbuf = rte_pktmbuf_clone(mbuf_ptr, mempool_ptr);
        if mbuf_ptr_clone.is_null() {
            return Err(Fail::new(libc::EINVAL, "cannot clone mbuf"));
        }

        Ok(mbuf_ptr_clone)
    }
}
