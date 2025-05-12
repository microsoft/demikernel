// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(non_camel_case_types)]

mod memory;
mod ops;
mod queue;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    memory::{demi_sgarray_t, demi_sgaseg_t, DEMI_SGARRAY_MAXLEN},
    ops::{demi_accept_result_t, demi_opcode_t, demi_qr_value_t, demi_qresult_t},
    queue::demi_qtoken_t,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A callback function.
pub type demi_callback_t = extern "C" fn(*const std::ffi::c_char, u32, u64);

/// Logging callback function.
pub type demi_log_callback_t = extern "C" fn(
    std::ffi::c_int,
    *const std::ffi::c_char,
    u32,
    *const std::ffi::c_char,
    u32,
    u32,
    *const std::ffi::c_char,
    u32,
);

pub type demi_metric_callback_t = extern "C" fn(u32, u32);

/// Demikernel Arguments
#[repr(C, packed)]
pub struct demi_args_t {
    pub argc: core::ffi::c_int,
    pub argv: *const *const core::ffi::c_char,
    pub callback: Option<demi_callback_t>,
    pub log_callback: Option<demi_log_callback_t>,
    pub metric_callback: Option<demi_metric_callback_t>,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum demi_metric_kind_t {
    DEMI_MK_EVENT = 1,
    DEMI_MK_SAMPLE = 2,
    DEMI_MK_RATE = 3,
}

#[repr(C)]
pub struct demi_metric_descriptor_t {
    pub id: u32,
    pub name: *const core::ffi::c_char,
    pub name_len: u32,
    pub description: *const core::ffi::c_char,
    pub description_len: u32,
    pub unit: *const core::ffi::c_char,
    pub unit_len: u32,
    pub kind: demi_metric_kind_t,
}

impl Default for demi_args_t {
    fn default() -> Self {
        Self {
            argc: 0,
            argv: std::ptr::null(),
            callback: None,
            log_callback: None,
            metric_callback: None,
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {

    /// Tests if the `DemiArgs` structure has the expected size.
    #[test]
    fn test_size_demi_args() -> Result<(), anyhow::Error> {
        // Size of a void pointer.
        const DEMIARGS_ARGC_SIZE: usize = 4;
        // Size of a c int.
        const DEMIARGS_ARGV_SIZE: usize = 8;
        // Size of a c char.
        const DEMIARGS_CALLBACK_SIZE: usize = 8;

        // The expected size of the `DemiArgs` structure.
        const DEMIARGS_SIZE: usize =
            DEMIARGS_ARGC_SIZE + DEMIARGS_ARGV_SIZE + DEMIARGS_CALLBACK_SIZE + DEMIARGS_CALLBACK_SIZE;

        // Check if the sizes match.
        assert_eq!(std::mem::size_of::<crate::runtime::types::demi_args_t>(), DEMIARGS_SIZE);

        Ok(())
    }
}
