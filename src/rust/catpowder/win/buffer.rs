// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::catpowder::win::umemreg::UmemReg;
use ::std::ops::{
    Deref,
    DerefMut,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct XdpBuffer {
    b: *mut xdp_rs::XSK_BUFFER_DESCRIPTOR,
    umemreg: UmemReg,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpBuffer {
    pub fn new(b: *mut xdp_rs::XSK_BUFFER_DESCRIPTOR, umemreg: UmemReg) -> Self {
        Self { b, umemreg }
    }

    pub fn len(&self) -> usize {
        unsafe { (*self.b).Length as usize }
    }

    pub fn set_len(&mut self, len: usize) {
        unsafe {
            (*self.b).Length = len as u32;
        }
    }

    unsafe fn base_address(&self) -> u64 {
        (*self.b).Address.__bindgen_anon_1.BaseAddress()
    }

    unsafe fn offset(&self) -> u64 {
        (*self.b).Address.__bindgen_anon_1.Offset()
    }

    unsafe fn addr(&self) -> *mut core::ffi::c_void {
        let mut ptr: *mut u8 = self.umemreg.get_address() as *mut u8;
        ptr = ptr.add(self.base_address() as usize);
        ptr = ptr.add(self.offset() as usize);
        ptr as *mut core::ffi::c_void
    }
}

impl Deref for XdpBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.addr() as *const u8, self.len()) }
    }
}

impl DerefMut for XdpBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.addr() as *mut u8, self.len()) }
    }
}
