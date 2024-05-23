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

/// UDP Configuration Descriptor
#[derive(Clone, Debug)]
pub struct UdpConfig {
    /// Offload Checksum to Hardware When Receiving?
    rx_checksum: bool,
    /// Offload Checksum to Hardware When Sending?
    tx_checksum: bool,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for UDP Configuration Descriptor
impl UdpConfig {
    /// Creates a UDP Configuration Descriptor.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let offload = config.udp_checksum_offload()?;
        Ok(Self {
            rx_checksum: offload,
            tx_checksum: offload,
        })
    }

    /// Gets the RX hardware checksum offload option in the target [UdpConfig].
    pub fn get_rx_checksum_offload(&self) -> bool {
        self.rx_checksum
    }

    /// Gets the XX hardware checksum offload option in the target [UdpConfig].
    pub fn get_tx_checksum_offload(&self) -> bool {
        self.tx_checksum
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Default Trait Implementation for UDP Configuration Descriptor
impl Default for UdpConfig {
    /// Creates a UDP Configuration Descriptor with the default values.
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

    /// Tests default instantiation for [UdpConfig].
    #[test]
    fn test_udp_config_default() -> Result<()> {
        let config: UdpConfig = UdpConfig::default();
        crate::ensure_eq!(config.get_rx_checksum_offload(), false);
        crate::ensure_eq!(config.get_tx_checksum_offload(), false);

        Ok(())
    }
}
