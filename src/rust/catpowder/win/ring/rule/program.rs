// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::{
        api::XdpApi,
        ring::rule::rule::XdpRule,
    },
    runtime::fail::Fail,
};
use ::windows::{
    core::{
        Error,
        HRESULT,
    },
    Win32::{
        Foundation,
        Foundation::HANDLE,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP program.
#[repr(C)]
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
        let mut handle: HANDLE = HANDLE::default();

        // Attempt to create the XDP program.
        if let Some(create_program) = api.get().XdpCreateProgram {
            let result: HRESULT = unsafe {
                create_program(
                    ifindex,
                    hookid,
                    queueid,
                    flags,
                    rule,
                    rule_count,
                    &mut handle as *mut HANDLE,
                )
            };
            let error: Error = Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(Self(handle)),
                false => Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XdpCreateProgram is not implemented");
            error!("new(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for XdpProgram {
    fn drop(&mut self) {
        if let Err(_) = unsafe { Foundation::CloseHandle(self.0) } {
            error!("drop(): Failed to close xdp program handle");
        }
    }
}
