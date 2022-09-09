// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::fail::Fail;
use ::eui48;
use ::libc::EINVAL;
use ::std::fmt;

//==============================================================================
// Structures
//==============================================================================

/// MAC Address
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct MacAddress(eui48::MacAddress);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for MAC Addresses
impl MacAddress {
    /// Creates a new MAC Address from an array of bytes.
    pub const fn new(bytes: [u8; 6]) -> Self {
        MacAddress(eui48::MacAddress::new(bytes))
    }

    /// Creates a new MAC address from a slice.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        MacAddress(eui48::MacAddress::from_bytes(bytes).unwrap())
    }

    /// Returns the array of bytes composing the target [MacAddress].
    pub fn octets(&self) -> [u8; 6] {
        self.0.to_array()
    }

    /// Returns a MAC Address that matches the broadcast one.
    pub fn broadcast() -> MacAddress {
        MacAddress(eui48::MacAddress::broadcast())
    }

    /// Returns a MAC Address that matches the null one.
    pub fn nil() -> MacAddress {
        MacAddress(eui48::MacAddress::nil())
    }

    /// Queries whether or not the target [MacAddress] is a null one.
    pub fn is_nil(self) -> bool {
        self.0.is_nil()
    }

    /// Queries whether or not the target [MacAddress] is a broadcast one.
    pub fn is_broadcast(self) -> bool {
        self.0.is_broadcast()
    }

    /// Queries whether or not the target [MacAddress] is a multicast one.
    pub fn is_multicast(self) -> bool {
        self.0.is_multicast()
    }

    /// Queries whether or not the target [MacAddress] is a unicast one.
    pub fn is_unicast(self) -> bool {
        self.0.is_unicast()
    }

    /// Converts the target [MacAddress] to a canonical representation.
    pub fn to_canonical(self) -> String {
        self.0.to_canonical()
    }

    /// Converts the target [MacAddress] to a byte-slice representation.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Creates a MAC Address from a canonical representation.
    pub fn parse_str(s: &str) -> Result<Self, Fail> {
        match eui48::MacAddress::parse_str(s) {
            Ok(addr) => Ok(Self(addr)),
            Err(_) => Err(Fail::new(EINVAL, "failed to parse MAC Address")),
        }
    }

    /// Converts the target [MacAddress] to an array of bytes.
    pub fn to_array(self) -> [u8; 6] {
        self.0.to_array()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Display Trait Implementation for MAC Addresses
impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Debug Trait Implementation for MAC Addresses
impl fmt::Debug for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MacAddress({})", &self.to_canonical())
    }
}
