// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types)]

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::pal::data_structures::SockAddr;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Maximum Length for Scatter-Gather Arrays
pub const DEMI_SGARRAY_MAXLEN: usize = 1;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Scatter-Gather Array Segment
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct demi_sgaseg_t {
    /// Underlying data.
    pub sgaseg_buf: *mut libc::c_void,
    /// Length of underlying data.
    pub sgaseg_len: u32,
}

/// Scatter-Gather Array
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct demi_sgarray_t {
    /// Reserved.
    pub sga_buf: *mut libc::c_void,
    /// Number of segments in this scatter-gather array.
    pub sga_numsegs: u32,
    /// Scatter-gather array segments.
    pub sga_segs: [demi_sgaseg_t; DEMI_SGARRAY_MAXLEN],
    /// Source address of the data contained in this scatter-gather array (if present).
    pub sga_addr: SockAddr,
}

#[cfg(test)]
mod test {

    use super::*;
    use std::mem;

    /// Tests if the `demi_sgaseg_t` structure has the expected size.
    #[test]
    fn test_size_demi_sgaseg_t() -> Result<(), anyhow::Error> {
        // Size of a void pointer.
        const SGASEG_BUF_SIZE: usize = 8;
        // Size of a u32.
        const SGASEG_LEN_SIZE: usize = 4;
        // Size of a demi_sgaseg_t structure.
        crate::ensure_eq!(mem::size_of::<demi_sgaseg_t>(), SGASEG_BUF_SIZE + SGASEG_LEN_SIZE);
        Ok(())
    }

    /// Tests if the `demi_sga_t` structure has the expected size.
    #[test]
    fn test_size_demi_sgarray_t() -> Result<(), anyhow::Error> {
        // Size of a void pointer.
        const SGA_BUF_SIZE: usize = 8;
        // Size of a u32.
        const SGA_NUMSEGS_SIZE: usize = 4;
        // Size of an array of demi_sgaseg_t structures.
        const SGA_SEGS_SIZE: usize = mem::size_of::<demi_sgaseg_t>() * DEMI_SGARRAY_MAXLEN;
        // Size of a SockAddr structure.
        const SGA_ADDR_SIZE: usize = mem::size_of::<SockAddr>();
        // Size of a demi_sgarray_t structure.
        crate::ensure_eq!(
            mem::size_of::<demi_sgarray_t>(),
            SGA_BUF_SIZE + SGA_NUMSEGS_SIZE + SGA_SEGS_SIZE + SGA_ADDR_SIZE
        );
        Ok(())
    }
}
