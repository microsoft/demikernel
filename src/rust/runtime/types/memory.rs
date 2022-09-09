// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types)]

//==============================================================================
// Imports
//==============================================================================

use ::libc::{
    c_void,
    sockaddr,
};

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
#[repr(C)]
#[derive(Copy, Clone)]
pub struct demi_sgarray_t {
    pub sga_buf: *mut c_void,
    pub sga_numsegs: u32,
    pub sga_segs: [demi_sgaseg_t; DEMI_SGARRAY_MAXLEN],
    pub sga_addr: sockaddr,
}
