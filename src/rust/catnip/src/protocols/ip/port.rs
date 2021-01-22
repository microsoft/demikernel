// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    fail::Fail,
    runtime::Runtime,
};
use std::{
    convert::TryFrom,
    num::NonZeroU16,
};

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
    ports: Vec<Port>,
}

impl EphemeralPorts {
    pub fn new<RT: Runtime>(rt: &RT) -> Self {
        let mut ports = (FIRST_PRIVATE_PORT..=65535u16)
            .map(|p| Port(NonZeroU16::new(p).unwrap()))
            .collect::<Vec<_>>();

        rt.rng_shuffle(&mut ports[..]);
        Self { ports }
    }

    pub fn alloc(&mut self) -> Result<Port, Fail> {
        self.ports.pop().ok_or(Fail::ResourceExhausted {
            details: "Out of private ports",
        })
    }

    pub fn free(&mut self, port: Port) {
        self.ports.push(port);
    }
}
