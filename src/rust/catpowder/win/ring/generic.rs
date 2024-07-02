// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP ring.
#[repr(C)]
pub struct XdpRing(xdp_rs::XSK_RING);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl XdpRing {
    /// Initializes a XDP ring.
    pub(super) fn new(info: &xdp_rs::XSK_RING_INFO) -> Self {
        Self(unsafe {
            let mut ring: xdp_rs::XSK_RING = std::mem::zeroed();
            xdp_rs::_XskRingInitialize(&mut ring, info);
            ring
        })
    }

    /// Reserves a consumer slot in the target ring.
    pub(super) fn consumer_reserve(&mut self, count: u32, idx: *mut u32) -> u32 {
        unsafe { xdp_rs::_XskRingConsumerReserve(&mut self.0, count, idx) }
    }

    /// Releases a consumer slot in the target ring.
    pub(super) fn consumer_release(&mut self, count: u32) {
        unsafe { xdp_rs::_XskRingConsumerRelease(&mut self.0, count) }
    }

    /// Reserves a producer slot in the target ring.
    pub(super) fn producer_reserve(&mut self, count: u32, idx: *mut u32) -> u32 {
        unsafe { xdp_rs::_XskRingProducerReserve(&mut self.0, count, idx) }
    }

    /// Submits a producer slot in the target ring.
    pub(super) fn producer_submit(&mut self, count: u32) {
        unsafe { xdp_rs::_XskRingProducerSubmit(&mut self.0, count) }
    }

    /// Gets the element at the target index.
    pub(super) fn get_element(&self, idx: u32) -> *mut std::ffi::c_void {
        unsafe { xdp_rs::_XskRingGetElement(&self.0, idx) }
    }
}
