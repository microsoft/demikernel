// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
#[cfg(not(debug_assertions))]
use ::rand::prelude::{SeedableRng, SliceRandom, SmallRng};
use ::std::collections::VecDeque;

//======================================================================================================================
// Constants
//======================================================================================================================

/// https://datatracker.ietf.org/doc/html/rfc6335
const FIRST_PRIVATE_PORT_NUMBER: u16 = 49152;
const LAST_PRIVATE_PORT_NUMBER: u16 = 65535;

/// Seed number for ephemeral port allocator.
#[cfg(not(debug_assertions))]
const EPHEMERAL_PORT_SEED: u64 = 12345;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct EphemeralPorts {
    port_numbers: VecDeque<u16>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl EphemeralPorts {
    pub fn is_private(port_number: u16) -> bool {
        port_number >= FIRST_PRIVATE_PORT_NUMBER
    }

    // Any port number will be allocated.
    pub fn alloc(&mut self) -> Result<u16, Fail> {
        self.port_numbers.pop_front().ok_or(Fail::new(
            libc::EADDRINUSE,
            "all port numbers in the ephemeral range are currently in use",
        ))
    }

    // A specific port number will be reserved, if available.
    pub fn reserve(&mut self, port_number: u16) -> Result<(), Fail> {
        if !self.port_numbers.contains(&port_number) {
            return Err(Fail::new(libc::ENOENT, "port_number not found"));
        }

        self.port_numbers.retain(|&p| p != port_number);

        Ok(())
    }

    pub fn free(&mut self, port_number: u16) -> Result<(), Fail> {
        if !Self::is_private(port_number) {
            let cause: String = format!("port_number {} is not in the ephemeral range", port_number);
            error!("free(): {}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        if self.port_numbers.contains(&port_number) {
            let cause: String = format!("port_number {} is already in the pool", port_number);
            error!("free(): {}", &cause);
            return Err(Fail::new(libc::EFAULT, &cause));
        }

        self.port_numbers.push_back(port_number);

        Ok(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for EphemeralPorts {
    fn default() -> Self {
        let mut port_numbers: Vec<u16> = Vec::<u16>::new();
        for port_number in (FIRST_PRIVATE_PORT_NUMBER..=LAST_PRIVATE_PORT_NUMBER).rev() {
            port_numbers.push(port_number);
        }
        #[cfg(not(debug_assertions))]
        {
            let mut rng: SmallRng = SmallRng::seed_from_u64(EPHEMERAL_PORT_SEED);
            port_numbers.shuffle(&mut rng);
        }
        Self {
            port_numbers: VecDeque::from(port_numbers),
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use crate::inetstack::protocols::layer4::ephemeral::{
        EphemeralPorts, FIRST_PRIVATE_PORT_NUMBER, LAST_PRIVATE_PORT_NUMBER,
    };
    use ::anyhow::Result;

    #[test]
    fn test_alloc_any_and_free() -> Result<()> {
        let mut port_numbers: EphemeralPorts = EphemeralPorts::default();

        let port_number: u16 = match port_numbers.alloc() {
            Ok(port_number) => port_number,
            Err(e) => anyhow::bail!("failed to allocate an ephemeral port ({:?})", &e),
        };

        if let Err(e) = port_numbers.free(port_number) {
            anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
        }

        Ok(())
    }

    #[test]
    fn test_alloc_specific_port_and_free() -> Result<()> {
        let mut port_numbers: EphemeralPorts = EphemeralPorts::default();

        if let Err(e) = port_numbers.reserve(FIRST_PRIVATE_PORT_NUMBER) {
            anyhow::bail!("failed to allocate an ephemeral port (error={:?})", &e);
        }

        if let Err(e) = port_numbers.free(FIRST_PRIVATE_PORT_NUMBER) {
            anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
        }

        Ok(())
    }

    #[test]
    fn test_alloc_and_free_all_ephemeral_ports() -> Result<()> {
        let mut port_numbers: EphemeralPorts = EphemeralPorts::default();

        for _ in FIRST_PRIVATE_PORT_NUMBER..=LAST_PRIVATE_PORT_NUMBER {
            if let Err(e) = port_numbers.alloc() {
                anyhow::bail!("failed to allocate an ephemeral port (error={:?})", &e);
            }
        }

        if port_numbers.alloc().is_ok() {
            anyhow::bail!("all ports should be allocated");
        }

        for port_number in FIRST_PRIVATE_PORT_NUMBER..=LAST_PRIVATE_PORT_NUMBER {
            if let Err(e) = port_numbers.free(port_number) {
                anyhow::bail!("failed to free ephemeral port (error={:?})", &e);
            }
        }

        Ok(())
    }

    #[test]
    fn test_reserve_and_free_all_ephemeral_ports() -> Result<()> {
        let mut port_numbers: EphemeralPorts = EphemeralPorts::default();

        for port_number in FIRST_PRIVATE_PORT_NUMBER..=LAST_PRIVATE_PORT_NUMBER {
            if let Err(e) = port_numbers.reserve(port_number) {
                anyhow::bail!(
                    "failed to allocate ephemeral port (port_number={:?}, error={:?})",
                    port_number,
                    &e
                );
            }
        }

        if port_numbers.alloc().is_ok() {
            anyhow::bail!("all ports should be allocated");
        }

        for port_number in FIRST_PRIVATE_PORT_NUMBER..=LAST_PRIVATE_PORT_NUMBER {
            if let Err(e) = port_numbers.free(port_number) {
                anyhow::bail!(
                    "failed to free ephemeral port (port_number={:?}, error={:?})",
                    port_number,
                    &e
                );
            }
        }

        Ok(())
    }

    #[test]
    fn test_free_unallocated_port() -> Result<()> {
        let mut port_numbers: EphemeralPorts = EphemeralPorts::default();

        if port_numbers.free(FIRST_PRIVATE_PORT_NUMBER).is_ok() {
            anyhow::bail!("freeing a port number that is not allocated should fail");
        }

        Ok(())
    }
}
