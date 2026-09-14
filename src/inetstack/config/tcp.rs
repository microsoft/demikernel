// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    inetstack::consts::{
        default_mss_from_mtu, DEFAULT_MSS, DEFAULT_MTU, FALLBACK_MSS, MAX_MSS, MIN_MSS, TCP_ACK_DELAY_TIMEOUT,
        TCP_HANDSHAKE_TIMEOUT,
    },
    runtime::fail::Fail,
};
use ::std::{cmp, time::Duration};

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

        // The MSS that we advertise in our SYN and SYN+ACK segments. An explicitly configured MSS always wins;
        // otherwise we derive it from the MTU, as prescribed by RFC 793 and RFC 6691.
        let mtu: usize = config.mtu().map_or(DEFAULT_MTU, |value: u16| value as usize);
        let derived_mss: usize = default_mss_from_mtu(mtu);

        match config.mss() {
            Ok(value) => {
                if !(MIN_MSS..=MAX_MSS).contains(&value) {
                    let cause: String = format!(
                        "parameter \"mss\" is out of range: {} (must be in {}..={})",
                        value, MIN_MSS, MAX_MSS
                    );
                    error!("TcpConfig::new(): {}", cause);
                    return Err(Fail::new(libc::ERANGE, &cause));
                }
                if value > derived_mss {
                    warn!(
                        "configured MSS ({}) exceeds what MTU ({}) can carry ({})",
                        value, mtu, derived_mss
                    );
                }
                options.advertised_mss = value;
            },
            // No MSS configured: derive it from the MTU.
            Err(_) => {
                options.advertised_mss = derived_mss;
                info!("no MSS configured, advertising MSS derived from MTU {}: {}", mtu, derived_mss);
            },
        }

        if let Ok(value) = config.tcp_checksum_offload() {
            options.rx_checksum_offload = value;
            options.tx_checksum_offload = value;
        }

        Ok(options)
    }

    /// Returns the MSS that we advertise to our peers.
    pub fn get_advertised_mss(&self) -> usize {
        self.advertised_mss
    }

    /// Returns the MSS to use for a connection whose peer advertised `received_mss`.
    ///
    /// The MSS option only tells us how large the segments that we *send* may be, so a peer is free to advertise an
    /// MSS that is larger than what our own interface can carry. Per RFC 1122 (Section 4.2.2.6), we must then use the
    /// smaller of the two values, otherwise the sender would build segments that do not fit in our transmit buffers.
    pub fn get_effective_mss(&self, received_mss: Option<usize>) -> usize {
        received_mss.map_or(FALLBACK_MSS, |mss: usize| cmp::min(mss, self.advertised_mss))
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
    use crate::{
        demikernel::config::Config,
        inetstack::{
            config::TcpConfig,
            consts::{default_mss_from_mtu, DEFAULT_MSS, DEFAULT_MTU, FALLBACK_MSS},
        },
    };
    use ::anyhow::{ensure, Result};
    use ::std::time::Duration;
    use ::yaml_rust::{Yaml, YamlLoader};

    /// Builds a [Config] out of an inline `inetstack_config` section.
    fn new_config(inetstack_config: &str) -> Config {
        let yaml: String = format!("inetstack_config:\n{}", inetstack_config);
        let docs: Vec<Yaml> = YamlLoader::load_from_str(&yaml).expect("should be valid yaml");
        Config(docs[0].clone())
    }

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

    /// The MSS should be derived from the MTU whenever it is not explicitly configured.
    #[test]
    fn test_tcp_config_mss_derived_from_mtu() -> Result<()> {
        // Standard Ethernet MTU: 1500 - 20 (IPv4) - 20 (TCP) = 1460.
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 1500\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), 1460);

        // Jumbo frames.
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 9000\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), 8960);

        // Azure's accelerated networking MTU.
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 4000\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), 3960);

        // An MTU that cannot carry a full 536-byte segment must not underflow: we clamp to the RFC 793 fallback.
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 576\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), 536);
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 20\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), FALLBACK_MSS);

        Ok(())
    }

    /// An explicitly configured MSS must take precedence over the value derived from the MTU.
    #[test]
    fn test_tcp_config_mss_overrides_mtu() -> Result<()> {
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 1500\n  mss: 1450\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), 1450);

        // Neither MTU nor MSS configured: we fall back to the MSS derived from DEFAULT_MTU.
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  arp_cache_ttl: 600\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), DEFAULT_MSS);
        crate::ensure_eq!(tcp_config.get_advertised_mss(), default_mss_from_mtu(DEFAULT_MTU));

        // An out-of-range MSS is rejected instead of panicking.
        ensure!(TcpConfig::new(&new_config("  mtu: 1500\n  mss: 535\n")).is_err());
        ensure!(TcpConfig::new(&new_config("  mtu: 1500\n  mss: 65536\n")).is_err());

        Ok(())
    }

    /// The MSS used for a connection must never exceed what we can actually send.
    #[test]
    fn test_tcp_config_effective_mss() -> Result<()> {
        let tcp_config: TcpConfig = TcpConfig::new(&new_config("  mtu: 1500\n"))?;
        crate::ensure_eq!(tcp_config.get_advertised_mss(), 1460);

        // Peer did not advertise an MSS: fall back to the RFC 793 value.
        crate::ensure_eq!(tcp_config.get_effective_mss(None), FALLBACK_MSS);

        // Peer advertised something smaller: honor it.
        crate::ensure_eq!(tcp_config.get_effective_mss(Some(536)), 536);

        // Peer advertised something larger (e.g., it sits on a jumbo-frame link): clamp it to our own MSS.
        crate::ensure_eq!(tcp_config.get_effective_mss(Some(8960)), 1460);

        Ok(())
    }
}
