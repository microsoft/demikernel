// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::std::convert::TryFrom;

// See https://www.iana.org/assignments/ieee-802-numbers/ieee-802-numbers.xhtml for more details.
const ETHERTYPE2_ARP: u16 = 0x806; // ARP Frames
const ETHERTYPE2_IPV4: u16 = 0x800; // IPv4 Frames
const ETHERTYPE2_IPV6: u16 = 0x86dd; // IPv6 Frames

//======================================================================================================================
// Structures
//======================================================================================================================

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum EtherType2 {
    Arp = ETHERTYPE2_ARP,
    Ipv4 = ETHERTYPE2_IPV4,
    Ipv6 = ETHERTYPE2_IPV6,
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl TryFrom<u16> for EtherType2 {
    type Error = Fail;

    /// Attempts to convert a [u16] into an [EtherType2].
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            ETHERTYPE2_ARP => Ok(EtherType2::Arp),
            ETHERTYPE2_IPV4 => Ok(EtherType2::Ipv4),
            ETHERTYPE2_IPV6 => Ok(EtherType2::Ipv6),
            _ => Err(Fail::new(libc::ENOTSUP, "unsupported ETHERTYPE")),
        }
    }
}
