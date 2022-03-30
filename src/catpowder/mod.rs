// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod runtime;
mod socket;

//==============================================================================
// Imports
//==============================================================================

use self::runtime::LinuxRuntime;
use crate::demikernel::config::Config;
use ::catnip::Catnip;
use ::runtime::fail::Fail;
use ::std::{
    ops::{
        Deref,
        DerefMut,
    },
    time::Instant,
};

//==============================================================================
// Structures
//==============================================================================

/// Catpowder LibOS
pub struct CatpowderLibOS(Catnip<LinuxRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catpowder LibOS
impl CatpowderLibOS {
    /// Instantiates a Catpowder LibOS.
    pub fn new() -> Self {
        let config_path: String = std::env::var("CONFIG_PATH").unwrap();
        let config: Config = Config::new(config_path);
        let rt: LinuxRuntime = LinuxRuntime::new(
            Instant::now(),
            config.local_link_addr,
            config.local_ipv4_addr,
            &config.local_interface_name,
            config.arp_table(),
        );
        let libos: Catnip<LinuxRuntime> = Catnip::new(rt).unwrap();
        CatpowderLibOS(libos)
    }

    // Gets address information.
    pub fn getaddrinfo(
        &self,
        _node: Option<&str>,
        _service: Option<&str>,
        _hints: Option<&libc::addrinfo>,
    ) -> Result<*mut libc::addrinfo, Fail> {
        unimplemented!();
    }

    // Releases address information.
    pub fn freeaddrinfo(&self, _ai: *mut libc::addrinfo) -> Result<(), Fail> {
        unimplemented!();
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Catpowder LibOS
impl Deref for CatpowderLibOS {
    type Target = Catnip<LinuxRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mutable De-Reference Trait Implementation for Catpowder LibOS
impl DerefMut for CatpowderLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
