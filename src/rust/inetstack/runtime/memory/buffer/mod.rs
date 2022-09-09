// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod databuffer;
#[cfg(feature = "libdpdk")]
mod dpdkbuffer;

//==============================================================================
// Imports
//==============================================================================

use ::core::ops::{
    Deref,
    DerefMut,
};
use ::std::fmt::Debug;

//==============================================================================
// Exports
//==============================================================================

pub use self::databuffer::DataBuffer;
#[cfg(feature = "libdpdk")]
pub use self::dpdkbuffer::DPDKBuffer;

//==============================================================================
// Enumerations
//==============================================================================

#[derive(Clone, Debug)]
pub enum Buffer {
    Heap(DataBuffer),
    #[cfg(feature = "libdpdk")]
    DPDK(DPDKBuffer),
}

//==============================================================================
// Associated Functions
//==============================================================================

impl Buffer {
    /// Removes bytes from the front of the target data buffer.
    pub fn adjust(&mut self, nbytes: usize) {
        match self {
            Buffer::Heap(dbuf) => dbuf.adjust(nbytes),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.adjust(nbytes),
        }
    }

    /// Removes bytes from the end of the target data buffer.
    pub fn trim(&mut self, nbytes: usize) {
        match self {
            Buffer::Heap(dbuf) => dbuf.trim(nbytes),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.trim(nbytes),
        }
    }
}

//==============================================================================
// Standard-Library Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Data Buffers
impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Buffer::Heap(dbuf) => dbuf.deref(),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.deref(),
        }
    }
}

/// Mutable De-Reference Trait Implementation for Data Buffers
impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Buffer::Heap(dbuf) => dbuf.deref_mut(),
            #[cfg(feature = "libdpdk")]
            Buffer::DPDK(mbuf) => mbuf.deref_mut(),
        }
    }
}
