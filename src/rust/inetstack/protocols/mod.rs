// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod layer1;
pub mod layer2;
pub mod layer3;
pub mod layer4;

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::slice::ChunksExact;

//======================================================================================================================
// Constants
//======================================================================================================================

// Maximum header size of all possible headers.
pub const MAX_HEADER_SIZE: usize =
    layer4::tcp::MAX_TCP_HEADER_SIZE + layer3::ipv4::IPV4_HEADER_MAX_SIZE as usize + layer2::ETHERNET2_HEADER_SIZE;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Computes the generic checksum of a bytes array.
///
/// This iterates all 16-bit array elements, summing
/// the values into a 32-bit variable. This functions
/// paddies with zero an octet at the end (if necessary)
/// to turn into a 16-bit element. Also, this may use
/// an initial value depending on the parameter `"start"`.
pub fn compute_generic_checksum(buf: &[u8], start: Option<u32>) -> u32 {
    let mut state: u32 = match start {
        Some(state) => state,
        None => 0xFFFF,
    };

    let mut chunks_iter: ChunksExact<u8> = buf.chunks_exact(2);
    while let Some(chunk) = chunks_iter.next() {
        state += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }

    if let Some(&b) = chunks_iter.remainder().get(0) {
        state += u16::from_be_bytes([b, 0]) as u32;
    }

    state
}

/// Folds 32-bit sum into 16-bit checksum value.
pub fn fold16(mut state: u32) -> u16 {
    while state > 0xFFFF {
        state -= 0xFFFF;
    }
    !state as u16
}
