// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::api::XdpApi;
use crate::runtime::fail::Fail;
use ::windows::{
    core::{
        Error,
        HRESULT,
    },
    Win32::Foundation::HANDLE,
};
use ::xdp_rs;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A XDP socket.
pub struct XdpSocket {
    pub socket: HANDLE,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

/// Associated functions for XDP sockets.
impl XdpSocket {
    pub fn create(api: &mut XdpApi) -> Result<Self, Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.endpoint();

        let mut socket: HANDLE = HANDLE::default();
        if let Some(create) = api.XskCreate {
            let result: HRESULT = unsafe { create(&mut socket) };
            let error: windows::core::Error = Error::from_hresult(result);
            match error.code().is_ok() {
                true => Ok(Self { socket }),
                false => Err(Fail::from(&error)),
            }
        } else {
            let cause: String = format!("XskCreate is not implemented");
            error!("create(): {:?}", &cause);
            Err(Fail::new(libc::ENOSYS, &cause))
        }
    }

    pub fn bind(&self, api: &mut XdpApi, ifindex: u32, queueid: u32, flags: i32) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.endpoint();

        if let Some(bind) = api.XskBind {
            let result: HRESULT = unsafe { bind(self.socket, ifindex, queueid, flags) };
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

    pub fn setsockopt(
        &mut self,
        api: &mut XdpApi,
        opt: u32,
        val: *const std::ffi::c_void,
        len: u32,
    ) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.endpoint();

        if let Some(setsocket) = api.XskSetSockopt {
            let result: HRESULT = unsafe { setsocket(self.socket, opt, val, len) };
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

    pub fn getsockopt(
        &self,
        api: &mut XdpApi,
        opt: u32,
        val: *mut std::ffi::c_void,
        len: *mut u32,
    ) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.endpoint();

        if let Some(getsockopt) = api.XskGetSockopt {
            let result: HRESULT = unsafe { getsockopt(self.socket, opt, val, len) };
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

    pub fn activate(&self, api: &mut XdpApi, flags: i32) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.endpoint();

        if let Some(activate) = api.XskActivate {
            let result: HRESULT = unsafe { activate(self.socket, flags) };
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

    pub fn notify_socket(
        &self,
        api: &mut XdpApi,
        flags: xdp_rs::XSK_NOTIFY_FLAGS,
        timeout: u32,
        result: *mut xdp_rs::XSK_NOTIFY_RESULT_FLAGS,
    ) -> Result<(), Fail> {
        let api: xdp_rs::XDP_API_TABLE = api.endpoint();

        if let Some(notify) = api.XskNotifySocket {
            let result: HRESULT = unsafe { notify(self.socket, flags, timeout, result) };
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
}

pub struct XdpRing {
    ring: xdp_rs::XSK_RING,
}

impl XdpRing {
    pub fn ring_initialize(info: &xdp_rs::XSK_RING_INFO) -> Self {
        let ring = unsafe {
            let mut ring: xdp_rs::XSK_RING = std::mem::zeroed();
            xdp_rs::_XskRingInitialize(&mut ring, info);
            ring
        };
        Self { ring }
    }

    pub fn ring_consumer_reserve(&mut self, count: u32, idx: *mut u32) -> u32 {
        unsafe { xdp_rs::_XskRingConsumerReserve(&mut self.ring, count, idx) }
    }

    pub fn ring_consumer_release(&mut self, count: u32) {
        unsafe { xdp_rs::_XskRingConsumerRelease(&mut self.ring, count) }
    }

    pub fn ring_producer_reserve(&mut self, count: u32, idx: *mut u32) -> u32 {
        unsafe { xdp_rs::_XskRingProducerReserve(&mut self.ring, count, idx) }
    }

    pub fn ring_producer_submit(&mut self, count: u32) {
        unsafe { xdp_rs::_XskRingProducerSubmit(&mut self.ring, count) }
    }

    pub fn ring_get_element(&self, idx: u32) -> *mut std::ffi::c_void {
        unsafe { xdp_rs::_XskRingGetElement(&self.ring, idx) }
    }
}
