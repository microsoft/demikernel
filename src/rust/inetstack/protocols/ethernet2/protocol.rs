// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::inetstack::runtime::fail::Fail;
use ::libc::ENOTSUP;
use ::num_traits::FromPrimitive;
use ::std::convert::TryFrom;

#[repr(u16)]
#[derive(FromPrimitive, Copy, Clone, PartialEq, Eq, Debug)]
pub enum EtherType2 {
    Arp = 0x806,
    Ipv4 = 0x800,
    Ipv6 = 0x86dd,
}

impl TryFrom<u16> for EtherType2 {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self, Fail> {
        match FromPrimitive::from_u16(n) {
            Some(n) => Ok(n),
            None => Err(Fail::new(ENOTSUP, "unsupported ETHERTYPE")),
        }
    }
}
