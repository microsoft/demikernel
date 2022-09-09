// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::runtime::fail::Fail;
use ::rand::prelude::{
    SliceRandom,
    SmallRng,
};

//==============================================================================
// Constants
//==============================================================================

const FIRST_PRIVATE_PORT: u16 = 49152;
const LAST_PRIVATE_PORT: u16 = 65535;

//==============================================================================
// Structures
//==============================================================================

pub struct EphemeralPorts {
    ports: Vec<u16>,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl EphemeralPorts {
    pub fn new(rng: &mut SmallRng) -> Self {
        let mut ports: Vec<u16> = Vec::<u16>::new();
        for port in FIRST_PRIVATE_PORT..LAST_PRIVATE_PORT {
            ports.push(port);
        }
        ports.shuffle(rng);
        Self { ports }
    }

    pub fn first_private_port() -> u16 {
        FIRST_PRIVATE_PORT
    }

    pub fn is_private(port: u16) -> bool {
        port >= FIRST_PRIVATE_PORT
    }

    pub fn alloc_any(&mut self) -> Result<u16, Fail> {
        self.ports.pop().ok_or(Fail::new(
            libc::EADDRINUSE,
            "all port numbers in the ephemeral port range are currently in use",
        ))
    }

    /// Allocates the specified port from the pool.
    pub fn alloc_port(&mut self, port: u16) -> Result<(), Fail> {
        // Check if port is not in the pool.
        if !self.ports.contains(&port) {
            return Err(Fail::new(libc::ENOENT, "port number not found"));
        }

        // Remove port from the pool.
        self.ports.retain(|&p| p != port);

        Ok(())
    }

    pub fn free(&mut self, port: u16) {
        self.ports.push(port);
    }
}
