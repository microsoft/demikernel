// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::ip::EphemeralPorts,
    runtime::fail::Fail,
};
use ::rand::{
    prelude::SmallRng,
    SeedableRng,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catloop Runtime
pub struct CatloopRuntime {
    /// Ephemeral port allocator.
    ephemeral_ports: EphemeralPorts,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Catloop Runtime. This data structure holds all of the cross-queue state for the Catloop libOS.
impl CatloopRuntime {
    pub fn new() -> Self {
        let mut rng: SmallRng = SmallRng::from_entropy();
        Self {
            ephemeral_ports: EphemeralPorts::new(&mut rng),
        }
    }

    /// Allocates an ephemeral port. If `port` is `Some(port)` then it tries to allocate `port`.
    pub fn alloc_ephemeral_port(&mut self, port: Option<u16>) -> Result<Option<u16>, Fail> {
        if let Some(port) = port {
            self.ephemeral_ports.alloc_port(port)?;
            Ok(None)
        } else {
            Ok(Some(self.ephemeral_ports.alloc_any()?))
        }
    }

    /// Releases an ephemeral `port`.
    pub fn free_ephemeral_port(&mut self, port: u16) -> Result<(), Fail> {
        self.ephemeral_ports.free(port)
    }
}
