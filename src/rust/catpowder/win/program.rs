// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        rule::XdpRule,
    },
    runtime::fail::Fail,
};
use ::windows::{
    core::HRESULT,
    Win32::{
        Foundation,
        Foundation::HANDLE,
    },
};
use ::xdp_rs;

//======================================================================================================================
// Structures
//======================================================================================================================

#[repr(C)]
#[derive(Default)]
pub struct XdpProgram(HANDLE);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpProgram {
    /// Creates a new XDP program.
    pub fn new(
        api: &mut XdpApi,
        rules: &[XdpRule],
        ifindex: u32,
        hookid: &xdp_rs::XDP_HOOK_ID,
        queueid: u32,
        flags: xdp_rs::XDP_CREATE_PROGRAM_FLAGS,
    ) -> Result<XdpProgram, Fail> {
        let rule: *const xdp_rs::XDP_RULE = rules.as_ptr() as *const xdp_rs::XDP_RULE;
        let rule_count: u32 = rules.len() as u32;
        let mut program: XdpProgram = XdpProgram::default();

        // Attempt to create the XDP program.
        if let Some(create_program) = api.endpoint().XdpCreateProgram {
            let result: HRESULT =
                unsafe { create_program(ifindex, hookid, queueid, flags, rule, rule_count, program.as_ptr()) };
            let error: windows::core::Error = windows::core::Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(program),
                false => Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XdpCreateProgram is not implemented");
            error!("new(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }

    /// Casts the target XDP program to a raw pointer.
    pub fn as_ptr(&mut self) -> *mut HANDLE {
        &mut self.0 as *mut HANDLE
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for XdpProgram {
    fn drop(&mut self) {
        let _ = unsafe { Foundation::CloseHandle(self.0) };
    }
}
