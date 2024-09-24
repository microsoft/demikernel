// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::libc::EINVAL;
use ::std::{fmt, str::FromStr};

//======================================================================================================================
// Structures
//======================================================================================================================

/// MAC Address
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct MacAddress(eui48::MacAddress);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl MacAddress {
    pub const fn new(bytes: [u8; 6]) -> Self {
        MacAddress(eui48::MacAddress::new(bytes))
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        MacAddress(eui48::MacAddress::from_bytes(bytes).unwrap())
    }

    /// Returns the array of bytes composing the target [MacAddress].
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

    pub fn is_multicast(self) -> bool {
        self.0.is_multicast()
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

    pub fn parse_canonical_str(canonical_macaddr_string: &str) -> Result<Self, Fail> {
        match eui48::MacAddress::parse_str(canonical_macaddr_string) {
            Ok(addr) => Ok(Self(addr)),
            Err(_) => Err(Fail::new(EINVAL, "failed to parse MAC Address")),
        }
    }

    /// Converts to a byte array.
    pub fn to_array(self) -> [u8; 6] {
        self.0.to_array()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

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

impl FromStr for MacAddress {
    type Err = Fail;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MacAddress::parse_canonical_str(s)
    }
}
