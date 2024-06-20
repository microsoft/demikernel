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
    memory::{
        demi_sgarray_t,
        demi_sgaseg_t,
        DEMI_SGARRAY_MAXLEN,
    },
    ops::{
        demi_accept_result_t,
        demi_opcode_t,
        demi_qr_value_t,
        demi_qresult_t,
    },
    queue::demi_qtoken_t,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A callback function.
pub type demi_callback_t = extern "C" fn(*const std::ffi::c_char, u32, u64);

/// Demikernel Arguments
#[repr(C, packed)]
pub struct demi_args_t {
    pub argc: core::ffi::c_int,
    pub argv: *const *const core::ffi::c_char,
    pub callback: Option<demi_callback_t>,
}

impl Default for demi_args_t {
    fn default() -> Self {
        Self {
            argc: 0,
            argv: std::ptr::null(),
            callback: None,
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
        const DEMIARGS_SIZE: usize = DEMIARGS_ARGC_SIZE + DEMIARGS_ARGV_SIZE + DEMIARGS_CALLBACK_SIZE;

        // Check if the sizes match.
        assert_eq!(std::mem::size_of::<crate::runtime::types::demi_args_t>(), DEMIARGS_SIZE);

        Ok(())
    }
}
