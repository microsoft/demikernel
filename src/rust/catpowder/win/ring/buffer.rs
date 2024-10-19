// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{catpowder::win::ring::umemreg::UmemReg, runtime::libxdp};
use ::std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
    vec::Vec,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct XdpBuffer {
    b: *mut libxdp::XSK_BUFFER_DESCRIPTOR,
    /// UMEM region that contains the buffer.
    umemreg: Rc<RefCell<UmemReg>>,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpBuffer {
    pub(super) fn new(b: *mut libxdp::XSK_BUFFER_DESCRIPTOR, umemreg: Rc<RefCell<UmemReg>>) -> Self {
        Self { b, umemreg }
    }

    pub(super) fn set_len(&mut self, len: usize) {
        unsafe {
            (*self.b).Length = len as u32;
        }
    }

    fn len(&self) -> usize {
        unsafe { (*self.b).Length as usize }
    }

    unsafe fn relative_base_address(&self) -> u64 {
        (*self.b).Address.__bindgen_anon_1.BaseAddress()
    }

    unsafe fn offset(&self) -> u64 {
        (*self.b).Address.__bindgen_anon_1.Offset()
    }

    unsafe fn compute_address(&self) -> *mut core::ffi::c_void {
        let mut ptr: *mut u8 = self.umemreg.borrow_mut().address() as *mut u8;
        ptr = ptr.add(self.relative_base_address() as usize);
        ptr = ptr.add(self.offset() as usize);
        ptr as *mut core::ffi::c_void
    }

    fn to_vector(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(self.len());
        self[..].clone_into(&mut out);
        out
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl From<XdpBuffer> for Vec<u8> {
    fn from(buffer: XdpBuffer) -> Vec<u8> {
        buffer.to_vector()
    }
}

impl Deref for XdpBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.compute_address() as *const u8, self.len()) }
    }
}

impl DerefMut for XdpBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.compute_address() as *mut u8, self.len()) }
    }
}
