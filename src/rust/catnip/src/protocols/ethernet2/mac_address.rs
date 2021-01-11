// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::fail::Fail;
use eui48;
use std::fmt;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct MacAddress(eui48::MacAddress);

impl MacAddress {
    pub const fn new(bytes: [u8; 6]) -> Self {
        MacAddress(eui48::MacAddress::new(bytes))
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        MacAddress(eui48::MacAddress::from_bytes(bytes).unwrap())
    }

    pub fn octets(&self) -> [u8; 6] {
        self.0.to_array()
    }

    pub fn broadcast() -> MacAddress {
        MacAddress(eui48::MacAddress::broadcast())
    }

    pub fn nil() -> MacAddress {
        MacAddress(eui48::MacAddress::nil())
    }

    pub fn is_nil(self) -> bool {
        self.0.is_nil()
    }

    pub fn is_broadcast(self) -> bool {
        self.0.is_broadcast()
    }

    pub fn is_unicast(self) -> bool {
        self.0.is_unicast()
    }

    pub fn to_canonical(self) -> String {
        self.0.to_canonical()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn parse_str(s: &str) -> Result<Self, Fail> {
        Ok(Self(eui48::MacAddress::parse_str(s)?))
    }

    pub fn to_array(self) -> [u8; 6] {
        self.0.to_array()
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MacAddress({})", &self.to_canonical())
    }
}
