// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::mbuf::Mbuf;
use crate::demikernel::dbuf::DataBuffer;
use ::runtime::memory::Buffer;
use ::std::ops::Deref;

//==============================================================================
// Enumerations
//==============================================================================

/// DPDK Buffer
#[derive(Clone, Debug)]
pub enum DPDKBuf {
    External(DataBuffer),
    Managed(Mbuf),
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Buffer Trait Implementation for DPDK Buffers
impl Buffer for DPDKBuf {
    /// Creates an empty [DPDKBuf].
    fn empty() -> Self {
        DPDKBuf::External(DataBuffer::empty())
    }

    /// Creates a [DPDKBuf] from a [u8] slice.
    fn from_slice(bytes: &[u8]) -> Self {
        DPDKBuf::External(DataBuffer::from_slice(bytes))
    }

    /// Removes `len` bytes at the beginning of the target [DPDKBuf].
    fn adjust(&mut self, num_bytes: usize) {
        match self {
            DPDKBuf::External(ref mut buf) => buf.adjust(num_bytes),
            DPDKBuf::Managed(ref mut mbuf) => mbuf.adjust(num_bytes),
        }
    }

    /// Removes `len` bytes at the end of the target [DPDKBuf].
    fn trim(&mut self, num_bytes: usize) {
        match self {
            DPDKBuf::External(ref mut buf) => buf.trim(num_bytes),
            DPDKBuf::Managed(ref mut mbuf) => mbuf.trim(num_bytes),
        }
    }
}

/// De-Reference Trait Implementation for DPDK Buffers
impl Deref for DPDKBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            DPDKBuf::External(ref buf) => buf.deref(),
            DPDKBuf::Managed(ref mbuf) => mbuf.deref(),
        }
    }
}
