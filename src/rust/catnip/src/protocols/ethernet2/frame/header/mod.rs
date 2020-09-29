// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(dead_code)]

#[cfg(test)]
mod tests;

use crate::protocols::ethernet2::MacAddress;
use byteorder::{ByteOrder, NetworkEndian};
use num_traits::FromPrimitive;
use crate::fail::Fail;
use std::convert::TryFrom;

pub const ETHERNET2_HEADER_SIZE: usize = 14;

#[repr(u16)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum EtherType {
    Arp = 0x806,
    Ipv4 = 0x800,
}

impl TryFrom<u16> for EtherType {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self, Fail> {
        match FromPrimitive::from_u16(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {
                details: "given ETHERTYPE is not supported",
            }),
        }
    }
}

impl Into<u16> for EtherType {
    fn into(self) -> u16 {
        self as u16
    }
}

pub struct Ethernet2Header<'a>(&'a [u8]);

impl<'a> Ethernet2Header<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        assert!(bytes.as_ref().len() == ETHERNET2_HEADER_SIZE);
        Ethernet2Header(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0
    }

    pub fn dest_addr(&self) -> MacAddress {
        MacAddress::from_bytes(&self.0[0..6])
    }

    pub fn src_addr(&self) -> MacAddress {
        MacAddress::from_bytes(&self.0[6..12])
    }

    pub fn ether_type(&self) -> Result<EtherType, Fail> {
        trace!("Ethernet2Header::ether_type()");
        let n = NetworkEndian::read_u16(&self.0[12..14]);
        Ok(EtherType::try_from(n)?)
    }
}

pub struct Ethernet2HeaderMut<'a>(&'a mut [u8]);

impl<'a> Ethernet2HeaderMut<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        assert!(bytes.as_ref().len() == ETHERNET2_HEADER_SIZE);
        Ethernet2HeaderMut(bytes)
    }

    pub fn as_bytes(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn dest_addr(&mut self, addr: MacAddress) {
        self.0[0..6].copy_from_slice(addr.as_bytes());
    }

    pub fn src_addr(&mut self, addr: MacAddress) {
        self.0[6..12].copy_from_slice(addr.as_bytes());
    }

    pub fn ether_type(&mut self, ether_type: EtherType) {
        NetworkEndian::write_u16(&mut self.0[12..14], ether_type.into());
    }
}
