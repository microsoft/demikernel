// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        network::consts::{
            DEFAULT_MSS,
            MAX_MSS,
            MIN_MSS,
            TCP_ACK_DELAY_TIMEOUT,
            TCP_HANDSHAKE_TIMEOUT,
        },
    },
};
use ::std::time::Duration;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Debug)]
pub struct TcpConfig {
    /// mss = Maximum Segment Size
    advertised_mss: usize,
    handshake_retries: usize,
    handshake_timeout: Duration,
    receive_window_size: u16,
    /// Scaling Factor for Window Size
    window_scale: u8,
    ack_delay_timeout: Duration,
    rx_checksum_offload: bool,
    tx_checksum_offload: bool,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TcpConfig {
    pub fn new(config: &Config) -> Result<Self, Fail> {
        let mut options = Self::default();

        if let Ok(value) = config.mss() {
            assert!(value >= MIN_MSS);
            assert!(value <= MAX_MSS);
            options.advertised_mss = value;
        }
        if let Ok(value) = config.tcp_checksum_offload() {
            options.rx_checksum_offload = value;
            options.tx_checksum_offload = value;
        }

        Ok(options)
    }

    pub fn get_advertised_mss(&self) -> usize {
        self.advertised_mss
    }

    pub fn get_handshake_retries(&self) -> usize {
        self.handshake_retries
    }

    pub fn get_handshake_timeout(&self) -> Duration {
        self.handshake_timeout
    }

    pub fn get_receive_window_size(&self) -> u16 {
        self.receive_window_size
    }

    pub fn get_window_scale(&self) -> u8 {
        self.window_scale
    }

    pub fn get_ack_delay_timeout(&self) -> Duration {
        self.ack_delay_timeout
    }

    pub fn get_tx_checksum_offload(&self) -> bool {
        self.tx_checksum_offload
    }

    pub fn get_rx_checksum_offload(&self) -> bool {
        self.rx_checksum_offload
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for TcpConfig {
    fn default() -> Self {
        TcpConfig {
            advertised_mss: DEFAULT_MSS,
            handshake_retries: 5,
            handshake_timeout: TCP_HANDSHAKE_TIMEOUT,
            receive_window_size: 0xffff,
            ack_delay_timeout: TCP_ACK_DELAY_TIMEOUT,
            window_scale: 0,
            rx_checksum_offload: false,
            tx_checksum_offload: false,
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::network::{
        config::TcpConfig,
        consts::DEFAULT_MSS,
    };
    use ::anyhow::Result;
    use ::std::time::Duration;

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
