// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catpowder::win::api::XdpApi,
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

/// A XDP socket.
#[repr(C)]
pub struct XdpSocket(HANDLE);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpSocket {
    /// Creates a XDP socket.
    pub fn create(api: &mut XdpApi) -> Result<Self, Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.get();

        let mut socket: HANDLE = HANDLE::default();
        if let Some(create) = api.XskCreate {
            let result: HRESULT = unsafe { create(&mut socket) };
            let error: windows::core::Error = Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(Self(socket)),
                false => Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XskCreate is not implemented");
            error!("create(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }

    /// Binds the target socket to a network interface and queue.
    pub fn bind(&self, api: &mut XdpApi, ifindex: u32, queueid: u32, flags: i32) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.get();

        if let Some(bind) = api.XskBind {
            let result: HRESULT = unsafe { bind(self.0, ifindex, queueid, flags) };
            let error: windows::core::Error = windows::core::Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(()),
                false => {
                    error!("bind(): {:?}", &error);
                    Err(Fail::from(&error))
                },
            }
        } else {
            let cause: String = format!("XskBind is not implemented");
            error!("bind(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }

    /// Set options in the target socket.
    pub fn setsockopt(
        &mut self,
        api: &mut XdpApi,
        opt: u32,
        val: *const std::ffi::c_void,
        len: u32,
    ) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.get();

        if let Some(setsocket) = api.XskSetSockopt {
            let result: HRESULT = unsafe { setsocket(self.0, opt, val, len) };
            let error: windows::core::Error = windows::core::Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(()),
                false => return Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XskSetSockopt is not implemented");
            error!("setsockopt(): {:?}", &cause);
            return Err(Fail::new(libc::ENOSYS, &cause));
        }
    }

    /// Get options from the target socket.
    pub fn getsockopt(
        &self,
        api: &mut XdpApi,
        opt: u32,
        val: *mut std::ffi::c_void,
        len: *mut u32,
    ) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.get();

        if let Some(getsockopt) = api.XskGetSockopt {
            let result: HRESULT = unsafe { getsockopt(self.0, opt, val, len) };
            let error: windows::core::Error = windows::core::Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(()),
                false => return Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XskGetSockopt is not implemented");
            error!("getsockopt(): {:?}", &cause);
            return Err(Fail::new(libc::ENOSYS, &cause));
        }
    }

    /// Activate the target socket.
    pub fn activate(&self, api: &mut XdpApi, flags: i32) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.get();

        if let Some(activate) = api.XskActivate {
            let result: HRESULT = unsafe { activate(self.0, flags) };
            let error: windows::core::Error = windows::core::Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(()),
                false => Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XskActivate is not implemented");
            error!("activate(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }

    /// Notifies the target socket about something.
    pub fn notify(
        &self,
        api: &mut XdpApi,
        flags: xdp_rs::XSK_NOTIFY_FLAGS,
        timeout: u32,
        result: *mut xdp_rs::XSK_NOTIFY_RESULT_FLAGS,
    ) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.get();

        if let Some(notify) = api.XskNotifySocket {
            let result: HRESULT = unsafe { notify(self.0, flags, timeout, result) };
            let error: windows::core::Error = windows::core::Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(()),
                false => Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XskNotifySocket is not implemented");
            error!("notify_socket(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }

    /// Converts the target socket into a raw handle.
    pub fn into_raw(&self) -> HANDLE {
        self.0
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for XdpSocket {
    fn drop(&mut self) {
        if let Err(_) = unsafe { Foundation::CloseHandle(self.0) } {
            error!("drop(): failed to close socket");
        }
    }
}
