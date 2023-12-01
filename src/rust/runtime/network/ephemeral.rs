// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
#[cfg(not(debug_assertions))]
use ::rand::prelude::{
    SeedableRng,
    SliceRandom,
    SmallRng,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// First private port. See https://datatracker.ietf.org/doc/html/rfc6335 for details.
const FIRST_PRIVATE_PORT: u16 = 49152;
/// Last private port. See https://datatracker.ietf.org/doc/html/rfc6335 for details.
const LAST_PRIVATE_PORT: u16 = 65535;
/// Seed number for ephemeral port allocator.
#[cfg(not(debug_assertions))]
const EPHEMERAL_PORT_SEED: u64 = 12345;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct EphemeralPorts {
    ports: Vec<u16>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl EphemeralPorts {
    /// Asserts wether a port is in the ephemeral port range.
    pub fn is_private(port: u16) -> bool {
        port >= FIRST_PRIVATE_PORT
    }

    /// Allocates any ephemeral port from the pool.
    pub fn alloc(&mut self) -> Result<u16, Fail> {
        self.ports.pop().ok_or(Fail::new(
            libc::EADDRINUSE,
            "all port numbers in the ephemeral port range are currently in use",
        ))
    }

    /// Allocates the specified port from the pool.
    pub fn reserve(&mut self, port: u16) -> Result<(), Fail> {
        // Check if port is not in the pool.
        if !self.ports.contains(&port) {
            return Err(Fail::new(libc::ENOENT, "port number not found"));
        }

        // Remove port from the pool.
        self.ports.retain(|&p| p != port);

        Ok(())
    }

    /// Releases a ephemeral port.
    pub fn free(&mut self, port: u16) -> Result<(), Fail> {
        // Check if port is in the valid range.
        if !Self::is_private(port) {
            let cause: String = format!("port {} is not in the ephemeral port range", port);
            error!("free(): {}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // Check if port is already in the pool.
        if self.ports.contains(&port) {
            let cause: String = format!("port {} is already in the pool", port);
            error!("free(): {}", &cause);
            return Err(Fail::new(libc::EFAULT, &cause));
        }

        self.ports.push(port);

        Ok(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for EphemeralPorts {
    /// Creates a new ephemeral port allocator.
    fn default() -> Self {
        let mut ports: Vec<u16> = Vec::<u16>::new();
        for port in (FIRST_PRIVATE_PORT..=LAST_PRIVATE_PORT).rev() {
            ports.push(port);
        }
        #[cfg(not(debug_assertions))]
        {
            let mut rng: SmallRng = SmallRng::seed_from_u64(EPHEMERAL_PORT_SEED);
            ports.shuffle(&mut rng);
        }
        Self { ports }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use super::{
        EphemeralPorts,
        FIRST_PRIVATE_PORT,
        LAST_PRIVATE_PORT,
    };
    use ::anyhow::Result;

    /// Attempts to allocate any ephemeral port and then release it.
    #[test]
    fn test_alloc_any_and_free() -> Result<()> {
        let mut ports: EphemeralPorts = EphemeralPorts::default();

        // Allocate a port.
        let port: u16 = match ports.alloc() {
            Ok(port) => port,
            Err(e) => anyhow::bail!("failed to allocate an ephemeral port ({:?})", &e),
        };

        // Free the port.
        if let Err(e) = ports.free(port) {
            anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
        }

        Ok(())
    }

    /// Attempts to allocate a specific ephemeral port and then release it.
    #[test]
    fn test_alloc_port_and_free() -> Result<()> {
        let mut ports: EphemeralPorts = EphemeralPorts::default();

        // Attempt to allocate a port.
        if let Err(e) = ports.reserve(FIRST_PRIVATE_PORT) {
            anyhow::bail!("failed to allocate an ephemeral port (error={:?})", &e);
        }

        // Free the port.
        if let Err(e) = ports.free(FIRST_PRIVATE_PORT) {
            anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
        }

        Ok(())
    }

    /// Attempts to allocate all ephemeral ports using [`EphemeralPorts::alloc_any`] and then release them.
    #[test]
    fn test_alloc_any_all() -> Result<()> {
        let mut ports: EphemeralPorts = EphemeralPorts::default();

        // Allocate all ports.
        for _ in FIRST_PRIVATE_PORT..=LAST_PRIVATE_PORT {
            if let Err(e) = ports.alloc() {
                anyhow::bail!("failed to allocate an ephemeral port (error={:?})", &e);
            }
        }

        // All ports should be allocated.
        if ports.alloc().is_ok() {
            anyhow::bail!("all ports should be allocated");
        }

        // Free all ports.
        for port in FIRST_PRIVATE_PORT..=LAST_PRIVATE_PORT {
            if let Err(e) = ports.free(port) {
                anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
            }
        }

        Ok(())
    }

    /// Attempts to allocate all ephemeral ports using [`EphemeralPorts::alloc_port`] and then release them.
    #[test]
    fn test_alloc_port_all() -> Result<()> {
        let mut ports: EphemeralPorts = EphemeralPorts::default();

        // Allocate all ports.
        for port in FIRST_PRIVATE_PORT..=LAST_PRIVATE_PORT {
            if let Err(e) = ports.reserve(port) {
                anyhow::bail!("failed to allocate an ephemeral port (port={:?}, error={:?})", port, &e);
            }
        }

        // All ports should be allocated.
        if ports.alloc().is_ok() {
            anyhow::bail!("all ports should be allocated");
        }

        // Free all ports.
        for port in FIRST_PRIVATE_PORT..=LAST_PRIVATE_PORT {
            if let Err(e) = ports.free(port) {
                anyhow::bail!("failed to free ephemeral port (port={:?}, error={:?})", port, &e);
            }
        }

        Ok(())
    }

    /// Attempts to release a port that is not managed by the ephemeral port allocator.
    #[test]
    fn test_free_unmanaged_port() -> Result<()> {
        let mut ports: EphemeralPorts = EphemeralPorts::default();

        // Attempt to free a port that is not in the pool.
        if ports.free(FIRST_PRIVATE_PORT).is_ok() {
            anyhow::bail!("freeing a port that is not managed by the ephemeral port allocator should fail");
        }

        Ok(())
    }

    /// Attempts to release a port that is not allocated.
    #[test]
    fn test_free_unallocated_port() -> Result<()> {
        let mut ports: EphemeralPorts = EphemeralPorts::default();

        // Attempt to free a port that is not in the pool.
        if ports.free(FIRST_PRIVATE_PORT).is_ok() {
            anyhow::bail!("freeing a port that is not allocated should fail");
        }

        Ok(())
    }
}
