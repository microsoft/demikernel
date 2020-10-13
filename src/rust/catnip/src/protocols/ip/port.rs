// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::fail::Fail;
use std::{
    convert::TryFrom,
    num::NonZeroU16,
};
use uniset::BitSet;

const FIRST_PRIVATE_PORT: u16 = 49152;

#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug, Display, Ord, PartialOrd)]
pub struct Port(NonZeroU16);

impl TryFrom<u16> for Port {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self, Fail> {
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

pub struct EphemeralPorts {
    bits: BitSet,
}

impl EphemeralPorts {
    pub fn new() -> Self {
        let num_ephemeral = 65535 - FIRST_PRIVATE_PORT;
        let mut bits = BitSet::with_capacity(num_ephemeral as usize);
        for i in 0..num_ephemeral {
            bits.set(i as usize);
        }
        Self { bits }
    }

    pub fn alloc(&mut self) -> Result<Port, Fail> {
        match self.bits.iter().next() {
            Some(i) => {
                self.bits.clear(i);
                Ok(Port(
                    NonZeroU16::new(FIRST_PRIVATE_PORT + i as u16).unwrap(),
                ))
            },
            None => Err(Fail::ResourceExhausted {
                details: "Out of private ports",
            }),
        }
    }

    pub fn free(&mut self, port: Port) {
        self.bits.set((port.0.get() - FIRST_PRIVATE_PORT) as usize)
    }
}
