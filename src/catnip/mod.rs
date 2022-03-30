// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::runtime::DPDKRuntime;
use crate::demikernel::config::Config;
use ::catnip::Catnip;
use ::dpdk_rs::load_mlx_driver;
use ::runtime::fail::Fail;
use ::std::ops::{
    Deref,
    DerefMut,
};

//==============================================================================
// Exports
//==============================================================================

pub use self::runtime::memory::DPDKBuf;

//==============================================================================
// Structures
//==============================================================================

/// Catnip LibOS
pub struct CatnipLibOS(Catnip<DPDKRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnip LibOS
impl CatnipLibOS {
    pub fn new() -> Self {
        load_mlx_driver();
        let config_path: String = std::env::var("CONFIG_PATH").unwrap();
        let config: Config = Config::new(config_path);
        let rt: DPDKRuntime = DPDKRuntime::new(
            config.local_ipv4_addr,
            &config.eal_init_args(),
            config.arp_table(),
            config.disable_arp,
            config.use_jumbo_frames,
            config.mtu,
            config.mss,
            config.tcp_checksum_offload,
            config.udp_checksum_offload,
        );
        let libos: Catnip<DPDKRuntime> = Catnip::new(rt).unwrap();
        CatnipLibOS(libos)
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

/// De-Reference Trait Implementation for Catnip LibOS
impl Deref for CatnipLibOS {
    type Target = Catnip<DPDKRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mutable De-Reference Trait Implementation for Catnip LibOS
impl DerefMut for CatnipLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
