// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::fail::Fail;
use ::libc::ERANGE;
use ::std::{
    convert::TryFrom,
    num::NonZeroU16,
};

//==============================================================================
// Structures
//==============================================================================

/// Port Number
#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Ord, PartialOrd)]
pub struct Port16(NonZeroU16);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Port Numbers
impl Port16 {
    /// Instantiates a port number.
    pub fn new(num: NonZeroU16) -> Self {
        Self { 0: num }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// From Trait Implementation for Port Numbers
impl From<Port16> for u16 {
    /// Converts the target [Port16] into a [u16].
    fn from(val: Port16) -> Self {
        u16::from(val.0)
    }
}

/// Try From Trait Implementation for Port Numbers
impl TryFrom<u16> for Port16 {
    type Error = Fail;

    /// Tries to convert the target [Port16] into a [u16].
    fn try_from(n: u16) -> Result<Self, Fail> {
        Ok(Port16(
            NonZeroU16::new(n).ok_or(Fail::new(ERANGE, "port number may not be zero"))?,
        ))
    }
}
