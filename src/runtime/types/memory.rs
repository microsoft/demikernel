// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Constants
//======================================================================================================================

use ::std::{mem, ptr};

/// Maximum Length for Scatter-Gather Arrays. Cannot be larger than u16::MAX
pub const DEMI_SGARRAY_MAXLEN: usize = 19;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Scatter-Gather Array Segment
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct demi_sgaseg_t {
    pub reserved_metadata_ptr: *mut libc::c_void,
    pub data_buf_ptr: *mut libc::c_void,
    pub data_len_bytes: u32,
}

/// Scatter-Gather Array
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct demi_sgarray_t {
    pub num_segments: u32,
    pub segments: [demi_sgaseg_t; DEMI_SGARRAY_MAXLEN],
    pub sockaddr_src: libc::sockaddr,
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for demi_sgaseg_t {
    fn default() -> Self {
        Self {
            reserved_metadata_ptr: ptr::null_mut() as *mut _,
            data_buf_ptr: ptr::null_mut() as *mut libc::c_void,
            data_len_bytes: 0,
        }
    }
}

impl Default for demi_sgarray_t {
    fn default() -> Self {
        Self {
            num_segments: 0,
            segments: [demi_sgaseg_t::default(); DEMI_SGARRAY_MAXLEN],
            sockaddr_src: unsafe { mem::zeroed() },
        }
    }
}
//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {

    use crate::runtime::types::memory::*;
    use std::mem;

    #[test]
    fn test_size_demi_sgaseg_t() -> Result<(), anyhow::Error> {
        // Size of a void pointer.
        const RESERVED_METADATA_SIZE: usize = 8;
        // Size of a void pointer.
        const DATA_BUF_PTR_SIZE: usize = 8;
        // Size of a u32.
        const DATA_LEN_SIZE: usize = 4;

        crate::ensure_eq!(
            mem::size_of::<demi_sgaseg_t>(),
            RESERVED_METADATA_SIZE + DATA_BUF_PTR_SIZE + DATA_LEN_SIZE
        );
        Ok(())
    }

    #[test]
    fn test_size_demi_sgarray_t() -> Result<(), anyhow::Error> {
        // Size of a u32.
        const NUM_SEGMENTS_SIZE: usize = 4;
        // Size of an array of demi_sgaseg_t structures.
        const ELEMENTS_SIZE: usize = mem::size_of::<demi_sgaseg_t>() * DEMI_SGARRAY_MAXLEN;
        // Size of a SockAddr structure.
        const SOCKADDR_SRC_SIZE: usize = 16;

        crate::ensure_eq!(
            mem::size_of::<demi_sgarray_t>(),
            NUM_SEGMENTS_SIZE + ELEMENTS_SIZE + SOCKADDR_SRC_SIZE
        );
        Ok(())
    }
}
