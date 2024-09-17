// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::fail::Fail,
};

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Debug)]
pub struct UdpConfig {
    rx_checksum: bool,
    tx_checksum: bool,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl UdpConfig {
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let offload = config.udp_checksum_offload()?;
        Ok(Self {
            rx_checksum: offload,
            tx_checksum: offload,
        })
    }

    pub fn get_rx_checksum_offload(&self) -> bool {
        self.rx_checksum
    }

    pub fn get_tx_checksum_offload(&self) -> bool {
        self.tx_checksum
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for UdpConfig {
    fn default() -> Self {
        UdpConfig {
            rx_checksum: false,
            tx_checksum: false,
        }
    }
}

//======================================================================================================================
// Structures
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::network::config::UdpConfig;
    use ::anyhow::Result;

    #[test]
    fn test_udp_config_default() -> Result<()> {
        let config: UdpConfig = UdpConfig::default();
        crate::ensure_eq!(config.get_rx_checksum_offload(), false);
        crate::ensure_eq!(config.get_tx_checksum_offload(), false);

        Ok(())
    }
}
