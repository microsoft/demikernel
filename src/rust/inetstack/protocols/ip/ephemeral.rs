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
    /// Creates a new ephemeral port allocator.
    pub fn new(rng: &mut SmallRng) -> Self {
        let mut ports: Vec<u16> = Vec::<u16>::new();
        for port in FIRST_PRIVATE_PORT..LAST_PRIVATE_PORT {
            ports.push(port);
        }
        ports.shuffle(rng);
        Self { ports }
    }

    /// Asserts wether a port is in the ephemeral port range.
    pub fn is_private(port: u16) -> bool {
        port >= FIRST_PRIVATE_PORT
    }

    /// Allocates any ephemeral port from the pool.
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

#[cfg(test)]
mod test {
    use super::{
        EphemeralPorts,
        FIRST_PRIVATE_PORT,
        LAST_PRIVATE_PORT,
    };
    use ::anyhow::Result;
    use ::rand::SeedableRng;
    use rand::prelude::SmallRng;

    /// Attempts to allocate any ephemeral port and then release it.
    #[test]
    fn test_alloc_any_and_free() -> Result<()> {
        let mut rng: SmallRng = SmallRng::seed_from_u64(0);
        let mut ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        // Allocate a port.
        let port: u16 = match ports.alloc_any() {
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
        let mut rng: SmallRng = SmallRng::seed_from_u64(0);
        let mut ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        // Attempt to allocate a port.
        if let Err(e) = ports.alloc_port(FIRST_PRIVATE_PORT) {
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
        let mut rng: SmallRng = SmallRng::seed_from_u64(0);
        let mut ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        // Allocate all ports.
        for _ in FIRST_PRIVATE_PORT..LAST_PRIVATE_PORT {
            if let Err(e) = ports.alloc_any() {
                anyhow::bail!("failed to allocate an ephemeral port (error={:?})", &e);
            }
        }

        // All ports should be allocated.
        if ports.alloc_any().is_ok() {
            anyhow::bail!("all ports should be allocated");
        }

        // Free all ports.
        for port in FIRST_PRIVATE_PORT..LAST_PRIVATE_PORT {
            if let Err(e) = ports.free(port) {
                anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
            }
        }

        Ok(())
    }

    /// Attempts to allocate all ephemeral ports using [`EphemeralPorts::alloc_port`] and then release them.
    #[test]
    fn test_alloc_port_all() -> Result<()> {
        let mut rng: SmallRng = SmallRng::seed_from_u64(0);
        let mut ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        // Allocate all ports.
        for port in FIRST_PRIVATE_PORT..LAST_PRIVATE_PORT {
            if let Err(e) = ports.alloc_port(port) {
                anyhow::bail!("failed to allocate an ephemeral port (port={:?}, error={:?})", port, &e);
            }
        }

        // All ports should be allocated.
        if ports.alloc_any().is_ok() {
            anyhow::bail!("all ports should be allocated");
        }

        // Free all ports.
        for port in FIRST_PRIVATE_PORT..LAST_PRIVATE_PORT {
            if let Err(e) = ports.free(port) {
                anyhow::bail!("failed to free ephemeral port (port={:?}, error={:?})", port, &e);
            }
        }

        Ok(())
    }

    /// Attempts to release a port that is not managed by the ephemeral port allocator.
    #[test]
    fn test_free_unmanaged_port() -> Result<()> {
        let mut rng: SmallRng = SmallRng::seed_from_u64(0);
        let mut ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        // Attempt to free a port that is not in the pool.
        if ports.free(FIRST_PRIVATE_PORT).is_ok() {
            anyhow::bail!("freeing a port that is not managed by the ephemeral port allocator should fail");
        }

        Ok(())
    }

    /// Attempts to release a port that is not allocated.
    #[test]
    fn test_free_unallocated_port() -> Result<()> {
        let mut rng: SmallRng = SmallRng::seed_from_u64(0);
        let mut ports: EphemeralPorts = EphemeralPorts::new(&mut rng);

        // Attempt to free a port that is not in the pool.
        if ports.free(FIRST_PRIVATE_PORT).is_ok() {
            anyhow::bail!("freeing a port that is not allocated should fail");
        }

        Ok(())
    }
}
