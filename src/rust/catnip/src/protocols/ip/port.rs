// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::prelude::*;
use std::num::NonZeroU16;

const FIRST_PRIVATE_PORT: u16 = 49152;

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Display)]
pub struct Port(NonZeroU16);

impl TryFrom<u16> for Port {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self> {
        Ok(Port(NonZeroU16::new(n).ok_or(Fail::OutOfRange {
            details: "port number may not be zero",
        })?))
    }
}

impl Into<u16> for Port {
    fn into(self) -> u16 {
        self.0.get()
    }
}

impl Port {
    pub fn first_private_port() -> Port {
        Port::try_from(FIRST_PRIVATE_PORT).unwrap()
    }

    pub fn is_private(self) -> bool {
        self.0.get() >= FIRST_PRIVATE_PORT
    }
}
