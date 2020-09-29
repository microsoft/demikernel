// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(dead_code)]

#[cfg(test)]
mod tests;

use byteorder::{ByteOrder, NetworkEndian};
use num_traits::FromPrimitive;
use crate::fail::Fail;
use std::convert::TryFrom;

pub const ICMPV4_HEADER_SIZE: usize = 4;

#[repr(u8)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum Icmpv4Type {
    EchoReply = 0,
    DestinationUnreachable = 3,
    EchoRequest = 8,
}

impl TryFrom<u8> for Icmpv4Type {
    type Error = Fail;

    fn try_from(n: u8) -> Result<Self, Fail> {
        match FromPrimitive::from_u8(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {
                details: "ICMPv4 echo type must be REQUEST or REPLY",
            }),
        }
    }
}

impl Into<u8> for Icmpv4Type {
    fn into(self) -> u8 {
        self as u8
    }
}

pub struct Icmpv4Header<'a>(&'a [u8]);

impl<'a> Icmpv4Header<'a> {
    pub fn new(bytes: &'a [u8]) -> Icmpv4Header<'a> {
        assert!(bytes.len() == ICMPV4_HEADER_SIZE);
        Icmpv4Header(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0
    }

    pub fn r#type(&self) -> Result<Icmpv4Type, Fail> {
        Ok(Icmpv4Type::try_from(self.0[0])?)
    }

    pub fn code(&self) -> u8 {
        self.0[1]
    }

    pub fn checksum(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[2..4])
    }
}

pub struct Icmpv4HeaderMut<'a>(&'a mut [u8]);

impl<'a> Icmpv4HeaderMut<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Icmpv4HeaderMut<'a> {
        assert!(bytes.len() == ICMPV4_HEADER_SIZE);
        Icmpv4HeaderMut(bytes)
    }

    pub fn as_bytes(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn r#type(&mut self, value: Icmpv4Type) {
        self.0[0] = value.into();
    }

    pub fn code(&mut self, value: u8) {
        self.0[1] = value
    }

    pub fn checksum(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[2..4], value)
    }
}
