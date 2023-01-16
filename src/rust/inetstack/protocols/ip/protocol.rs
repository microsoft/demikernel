// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::std::convert::TryFrom;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Ipv4 Protocol
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IpProtocol {
    /// Internet Control Message Protocol
    ICMPv4 = 0x01,
    /// Transmission Control Protocol
    TCP = 0x06,
    /// User Datagram Protocol
    UDP = 0x11,
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// TryFrom trait implementation.
impl TryFrom<u8> for IpProtocol {
    type Error = Fail;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(IpProtocol::ICMPv4),
            0x06 => Ok(IpProtocol::TCP),
            0x11 => Ok(IpProtocol::UDP),
            _ => Err(Fail::new(libc::ENOTSUP, "unsupported IPv4 protocol")),
        }
    }
}
