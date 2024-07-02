// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::std::ptr;
use ::windows::core::{
    Error,
    HRESULT,
};
use ::xdp_rs;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for an XDP API.
#[repr(C)]
pub struct XdpApi(*const xdp_rs::XDP_API_TABLE);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpApi {
    /// Opens a new XDP API endpoint.
    pub fn new() -> Result<Self, Fail> {
        let mut api: *const xdp_rs::XDP_API_TABLE = ptr::null_mut();

        let result: HRESULT = unsafe { xdp_rs::XdpOpenApi(xdp_rs::XDP_API_VERSION_1, &mut api) };

        let error: windows::core::Error = Error::from_hresult(result);
        match error.code().is_ok() {
            true => Ok(Self(api)),
            false => Err(Fail::from(&error)),
        }
    }

    pub fn get(&self) -> xdp_rs::XDP_API_TABLE {
        unsafe {
            let api: *const xdp_rs::XDP_API_TABLE = self.0;
            *api
        }
    }
}

impl Drop for XdpApi {
    fn drop(&mut self) {
        let api: xdp_rs::XDP_API_TABLE = unsafe {
            let api: *const xdp_rs::XDP_API_TABLE = self.0;
            *api
        };

        if let Some(close) = api.XdpCloseApi {
            unsafe { close(self.0) };
        }
    }
}
