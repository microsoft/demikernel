// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::runtime::libxdp;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP user memory region.
pub struct UmemReg {
    _buffer: Vec<u8>,
    umem: libxdp::XSK_UMEM_REG,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl UmemReg {
    /// Creates a new XDP user memory region with `count` blocks of `chunk_size` bytes.
    pub fn new(count: u32, chunk_size: u32) -> Self {
        let total_size: u64 = count as u64 * chunk_size as u64;
        let mut buffer: Vec<u8> = Vec::<u8>::with_capacity(total_size as usize);

        let umem: libxdp::XSK_UMEM_REG = libxdp::XSK_UMEM_REG {
            TotalSize: total_size,
            ChunkSize: chunk_size,
            Headroom: 0,
            Address: buffer.as_mut_ptr() as *mut core::ffi::c_void,
        };

        Self { _buffer: buffer, umem }
    }

    /// Gets a reference to the underlying XDP user memory region.
    pub fn as_ref(&self) -> &libxdp::XSK_UMEM_REG {
        &self.umem
    }

    /// Returns a raw pointer to the the start address of the user memory region.
    pub fn address(&self) -> *mut core::ffi::c_void {
        self.umem.Address
    }
}
