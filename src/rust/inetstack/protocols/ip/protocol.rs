// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::inetstack::runtime::fail::Fail;
use ::libc::ENOTSUP;
use ::num_traits::FromPrimitive;
use ::std::convert::TryFrom;

//==============================================================================
// Structures
//==============================================================================

/// Ipv4 Protocol
#[repr(u8)]
#[derive(FromPrimitive, Copy, Clone, PartialEq, Eq, Debug)]
pub enum IpProtocol {
    /// Internet Control Message Protocol
    ICMPv4 = 0x01,
    /// Transmission Control Protocol
    TCP = 0x06,
    /// User Datagram Protocol
    UDP = 0x11,
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// TryFrom trait implementation.
impl TryFrom<u8> for IpProtocol {
    type Error = Fail;

    fn try_from(n: u8) -> Result<Self, Fail> {
        match FromPrimitive::from_u8(n) {
            Some(n) => Ok(n),
            None => Err(Fail::new(ENOTSUP, "unsupported IPv4 protocol")),
        }
    }
}
