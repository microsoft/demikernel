// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::network::consts::{
    DEFAULT_MSS,
    MAX_MSS,
    MIN_MSS,
};
use ::std::time::Duration;

//==============================================================================
// Structures
//==============================================================================

/// TCP Configuration Descriptor
#[derive(Clone, Debug)]
pub struct TcpConfig {
    /// Advertised Maximum Segment Size
    advertised_mss: usize,
    /// Number of Retries for TCP Handshake Algorithm
    handshake_retries: usize,
    /// Timeout for TCP Handshake Algorithm
    handshake_timeout: Duration,
    /// Window Size
    receive_window_size: u16,
    /// Scaling Factor for Window Size
    window_scale: u8,
    /// Timeout for Delayed ACKs
    ack_delay_timeout: Duration,
    /// Offload Checksum to Hardware When Receiving?
    rx_checksum_offload: bool,
    /// Offload Checksum to Hardware When Sending?
    tx_checksum_offload: bool,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for TCP Configuration Descriptor
impl TcpConfig {
    /// Creates a TCP Configuration Descriptor.
    pub fn new(
        advertised_mss: Option<usize>,
        handshake_retries: Option<usize>,
        handshake_timeout: Option<Duration>,
        receive_window_size: Option<u16>,
        window_scale: Option<u8>,
        ack_delay_timeout: Option<Duration>,
        rx_checksum_offload: Option<bool>,
        tx_checksum_offload: Option<bool>,
    ) -> Self {
        let mut options = Self::default();

        if let Some(value) = advertised_mss {
            options = options.set_advertised_mss(value);
        }
        if let Some(value) = handshake_retries {
            options = options.set_handshake_retries(value);
        }
        if let Some(value) = handshake_timeout {
            options = options.set_handshake_timeout(value);
        }
        if let Some(value) = receive_window_size {
            options = options.set_receive_window_size(value);
        }
        if let Some(value) = window_scale {
            options = options.set_window_scale(value);
        }
        if let Some(value) = ack_delay_timeout {
            options = options.set_ack_delay_timeout(value);
        }
        if let Some(value) = rx_checksum_offload {
            options.rx_checksum_offload = value;
        }
        if let Some(value) = tx_checksum_offload {
            options.tx_checksum_offload = value;
        }

        options
    }

    /// Gets the advertised maximum segment size in the target [TcpConfig].
    pub fn get_advertised_mss(&self) -> usize {
        self.advertised_mss
    }

    /// Gets the number of TCP handshake retries in the target [TcpConfig].
    pub fn get_handshake_retries(&self) -> usize {
        self.handshake_retries
    }

    /// Gets the handshake TCP timeout in the target [TcpConfig].
    pub fn get_handshake_timeout(&self) -> Duration {
        self.handshake_timeout
    }

    /// Gets the receiver window size in the target [TcpConfig].
    pub fn get_receive_window_size(&self) -> u16 {
        self.receive_window_size
    }

    /// Gets the window scale in the target [TcpConfig]
    pub fn get_window_scale(&self) -> u8 {
        self.window_scale
    }

    /// Gets the acknowledgement delay timeout in the target [TcpConfig].
    pub fn get_ack_delay_timeout(&self) -> Duration {
        self.ack_delay_timeout
    }

    /// Gets the TX hardware checksum offload option in the target [TcpConfig].
    pub fn get_tx_checksum_offload(&self) -> bool {
        self.tx_checksum_offload
    }

    /// Gets the RX hardware checksum offload option in the target [TcpConfig].
    pub fn get_rx_checksum_offload(&self) -> bool {
        self.rx_checksum_offload
    }

    /// Sets the advertised maximum segment size in the target [TcpConfig].
    fn set_advertised_mss(mut self, value: usize) -> Self {
        assert!(value >= MIN_MSS);
        assert!(value <= MAX_MSS);
        self.advertised_mss = value;
        self
    }

    /// Sets the number of TCP handshake retries in the target [TcpConfig].
    fn set_handshake_retries(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.handshake_retries = value;
        self
    }

    /// Sets the handshake TCP timeout in the target [TcpConfig].
    fn set_handshake_timeout(mut self, value: Duration) -> Self {
        assert!(value > Duration::new(0, 0));
        self.handshake_timeout = value;
        self
    }

    /// Sets the receiver window size in the target [TcpConfig].
    fn set_receive_window_size(mut self, value: u16) -> Self {
        assert!(value > 0);
        self.receive_window_size = value;
        self
    }

    /// Gets the window scale in the target [TcpConfig]
    fn set_window_scale(mut self, value: u8) -> Self {
        self.window_scale = value;
        self
    }

    /// Sets the acknowledgement delay timeout in the target [TcpConfig].
    fn set_ack_delay_timeout(mut self, value: Duration) -> Self {
        assert!(value <= Duration::from_millis(500));
        self.ack_delay_timeout = value;
        self
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Default Trait Implementation for TCP Configuration Descriptor
impl Default for TcpConfig {
    /// Creates a TCP Configuration Descriptor with the default values.
    fn default() -> Self {
        TcpConfig {
            advertised_mss: DEFAULT_MSS,
            handshake_retries: 5,
            handshake_timeout: Duration::from_secs(3),
            receive_window_size: 0xffff,
            ack_delay_timeout: Duration::from_millis(5),
            window_scale: 0,
            rx_checksum_offload: false,
            tx_checksum_offload: false,
        }
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::network::{
        config::TcpConfig,
        consts::DEFAULT_MSS,
    };
    use ::anyhow::Result;
    use ::std::time::Duration;

    /// Tests default instantiation for [UdpConfig].
    #[test]
    fn test_tcp_config_default() -> Result<()> {
        let config: TcpConfig = TcpConfig::default();
        crate::ensure_eq!(config.get_advertised_mss(), DEFAULT_MSS);
        crate::ensure_eq!(config.get_handshake_retries(), 5);
        crate::ensure_eq!(config.get_handshake_timeout(), Duration::from_secs(3));
        crate::ensure_eq!(config.get_receive_window_size(), 0xffff);
        crate::ensure_eq!(config.get_window_scale(), 0);
        crate::ensure_eq!(config.get_rx_checksum_offload(), false);
        crate::ensure_eq!(config.get_tx_checksum_offload(), false);

        Ok(())
    }
}
