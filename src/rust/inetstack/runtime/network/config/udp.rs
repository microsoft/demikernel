// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Constants & Structures
//==============================================================================

/// UDP Configuration Descriptor
#[derive(Clone, Debug)]
pub struct UdpConfig {
    /// Offload Checksum to Hardware When Receiving?
    rx_checksum: bool,
    /// Offload Checksum to Hardware When Sending?
    tx_checksum: bool,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate functions for UDP Configuration Descriptor
impl UdpConfig {
    /// Creates a UDP Configuration Descriptor.
    pub fn new(rx_checksum: Option<bool>, tx_checksum: Option<bool>) -> Self {
        let mut config = Self::default();
        if let Some(rx_checksum) = rx_checksum {
            config.set_rx_checksum_offload(rx_checksum);
        }
        if let Some(tx_checksum) = tx_checksum {
            config.set_tx_checksum_offload(tx_checksum);
        }
        config
    }

    /// Gets the RX hardware checksum offload option in the target [UdpConfig].
    pub fn get_rx_checksum_offload(&self) -> bool {
        self.rx_checksum
    }

    /// Gets the XX hardware checksum offload option in the target [UdpConfig].
    pub fn get_tx_checksum_offload(&self) -> bool {
        self.tx_checksum
    }

    /// Sets the RX hardware checksum offload option in the target [UdpConfig].
    fn set_rx_checksum_offload(&mut self, rx_checksum: bool) {
        self.rx_checksum = rx_checksum;
    }

    /// Sets the TX hardware checksum offload option in the target [UdpConfig].
    fn set_tx_checksum_offload(&mut self, tx_checksum: bool) {
        self.tx_checksum = tx_checksum;
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

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

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use super::UdpConfig;

    /// Tests default instantiation for [UdpConfig].
    #[test]
    fn test_udp_config_default() {
        let config: UdpConfig = UdpConfig::default();
        assert!(!config.get_rx_checksum_offload());
        assert!(!config.get_tx_checksum_offload());
    }

    /// Tests custom instantiation for [UdpConfig].
    #[test]
    fn test_udp_config_custom() {
        let config: UdpConfig = UdpConfig::new(Some(true), Some(true));
        assert!(config.get_rx_checksum_offload());
        assert!(config.get_tx_checksum_offload());
    }
}
