// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::{marker::PhantomData, mem::MaybeUninit};

use crate::runtime::libxdp;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP ring.
#[repr(C)]
pub struct XdpRing<T>(libxdp::XSK_RING, PhantomData<T>);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl<T> XdpRing<T> {
    /// Initializes a XDP ring.
    pub(super) fn new(info: &libxdp::XSK_RING_INFO) -> Self {
        let ring: libxdp::XSK_RING = unsafe {
            let mut ring: libxdp::XSK_RING = std::mem::zeroed();
            libxdp::_XskRingInitialize(&mut ring, info);
            ring
        };

        if !ring.SharedElements.cast::<T>().is_aligned() {
            panic!("XdpRing::new(): ring memory is not aligned for type T");
        }

        Self(ring, PhantomData)
    }

    /// Reserves a consumer slot in the target ring.
    pub(super) fn consumer_reserve(&mut self, count: u32, idx: &mut u32) -> u32 {
        unsafe { libxdp::_XskRingConsumerReserve(&mut self.0, count, idx as *mut u32) }
    }

    /// Releases a consumer slot in the target ring.
    pub(super) fn consumer_release(&mut self, count: u32) {
        unsafe { libxdp::_XskRingConsumerRelease(&mut self.0, count) }
    }

    /// Reserves a producer slot in the target ring.
    pub(super) fn producer_reserve(&mut self, count: u32, idx: &mut u32) -> u32 {
        unsafe { libxdp::_XskRingProducerReserve(&mut self.0, count, idx as *mut u32) }
    }

    /// Submits a producer slot in the target ring.
    pub(super) fn producer_submit(&mut self, count: u32) {
        unsafe { libxdp::_XskRingProducerSubmit(&mut self.0, count) }
    }

    /// Gets the element at the target index.
    pub(super) fn get_element(&self, idx: u32) -> &mut MaybeUninit<T> {
        // Safety: the alignment of ring elements is validated by the constructor. We rely on the XDP runtime to
        // provide valid memory for the ring.
        unsafe { &mut *libxdp::_XskRingGetElement(&self.0, idx).cast() }
    }

    pub(super) fn needs_poke(&self) -> bool {
        unsafe { libxdp::_XskRingProducerNeedPoke(&self.0) != 0 }
    }

    pub(super) fn has_error(&self) -> bool {
        unsafe { libxdp::_XskRingError(&self.0) != 0 }
    }
}
