// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types)]

//==============================================================================
// Imports
//==============================================================================

use ::libc::c_void;

use crate::pal::data_structures::SockAddr;

//==============================================================================
// Constants
//==============================================================================

/// Maximum Length for Scatter-Gather Arrays
pub const DEMI_SGARRAY_MAXLEN: usize = 1;

//==============================================================================
// Structures
//==============================================================================

/// Scatter-Gather Array Segment
#[repr(C)]
#[derive(Copy, Clone)]
pub struct demi_sgaseg_t {
    /// Underlying data.
    pub sgaseg_buf: *mut c_void,
    /// Length of underlying data.
    pub sgaseg_len: u32,
}

/// Scatter-Gather Array
// ToDo: Review the inclusion of the sga_addr field (only used for recvfrom?) in this structure.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct demi_sgarray_t {
    /// Reserved.
    pub sga_buf: *mut c_void,
    /// Number of segments in this scatter-gather array.
    pub sga_numsegs: u32,
    /// Scatter-gather array segments.
    pub sga_segs: [demi_sgaseg_t; DEMI_SGARRAY_MAXLEN],
    /// Source address of the data contained in this scatter-gather array (if present).
    pub sga_addr: SockAddr,
}
