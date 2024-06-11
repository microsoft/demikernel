// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub struct XdpBuffer {
    b: *const xdp_rs::XSK_BUFFER_DESCRIPTOR,
}

impl XdpBuffer {
    pub fn new(b: *const xdp_rs::XSK_BUFFER_DESCRIPTOR) -> Self {
        Self { b }
    }

    pub fn addr(&self) -> u64 {
        unsafe { (*self.b).Address.__bindgen_anon_1.BaseAddress() }
    }

    pub fn len(&self) -> usize {
        unsafe { (*self.b).Length as usize }
    }
}
