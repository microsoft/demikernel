// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP user memory region.
#[repr(C)]
#[derive(Clone)]
pub struct UmemReg(xdp_rs::XSK_UMEM_REG);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl UmemReg {
    /// Creates a new XDP user memory region with `count` blocks of `chunk_size` bytes.
    pub fn new(count: u32, chunk_size: u32) -> Self {
        let total_size: u64 = count as u64 * chunk_size as u64;
        let mut buffer: Vec<u8> = Vec::<u8>::with_capacity(total_size as usize);

        let mem: xdp_rs::XSK_UMEM_REG = xdp_rs::XSK_UMEM_REG {
            TotalSize: total_size,
            ChunkSize: chunk_size,
            Headroom: 0,
            Address: buffer.as_mut_ptr() as *mut core::ffi::c_void,
        };

        Self(mem)
    }

    /// Gets a reference to the underlying XDP user memory region.
    pub fn as_ref(&self) -> &xdp_rs::XSK_UMEM_REG {
        &self.0
    }

    /// Returns a raw pointer to the the start address of the user memory region.
    pub fn address(&self) -> *mut core::ffi::c_void {
        self.0.Address
    }
}
