// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::ops::Deref;

use super::umemreg::UmemReg;

pub struct XdpBuffer {
    b: *mut xdp_rs::XSK_BUFFER_DESCRIPTOR,
    umemreg: UmemReg,
}

impl XdpBuffer {
    pub fn new(b: *mut xdp_rs::XSK_BUFFER_DESCRIPTOR, umemreg: UmemReg) -> Self {
        Self { b, umemreg }
    }

    pub fn addr(&self) -> u64 {
        unsafe { (*self.b).Address.__bindgen_anon_1.BaseAddress() }
    }

    pub fn len(&self) -> usize {
        unsafe { (*self.b).Length as usize }
    }

    pub fn set_len(&mut self, len: usize) {
        unsafe {
            (*self.b).Length = len as u32;
        }
    }

    pub fn get_addr(&self) -> *mut core::ffi::c_void {
        self.umemreg.get_address()
    }
}

impl Deref for XdpBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.get_addr() as *const u8, self.len()) }
    }
}
