// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::windows::core::HRESULT;
use ::xdp_rs;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone)]
pub struct XdpApi {
    pub endpoint: *const xdp_rs::XDP_API_TABLE,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpApi {
    pub fn new() -> Result<Self, Fail> {
        let mut api: *const xdp_rs::XDP_API_TABLE = std::ptr::null_mut();

        let result: HRESULT = unsafe { xdp_rs::XdpOpenApi(xdp_rs::XDP_API_VERSION_1, &mut api) };

        let error: windows::core::Error = windows::core::Error::from_hresult(result);
        match error.code().is_ok() {
            true => Ok(Self { endpoint: api }),
            false => Err(Fail::from(&error)),
        }
    }

    pub fn endpoint(&self) -> xdp_rs::XDP_API_TABLE {
        unsafe {
            let api: *const xdp_rs::XDP_API_TABLE = self.endpoint;
            *api
        }
    }
}

impl Drop for XdpApi {
    fn drop(&mut self) {
        let api: xdp_rs::XDP_API_TABLE = unsafe {
            let api: *const xdp_rs::XDP_API_TABLE = self.endpoint;
            *api
        };

        if let Some(close) = api.XdpCloseApi {
            unsafe { close(self.endpoint) };
        }
    }
}
