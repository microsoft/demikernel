// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        network::types::MacAddress,
    },
};
use ::std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::Duration,
};

//==============================================================================
// Structures
//==============================================================================

/// ARP Configuration Descriptor
#[derive(Clone, Debug)]
pub struct ArpConfig {
    /// Time to Live for ARP Cache
    cache_ttl: Duration,
    /// Timeout for ARP Requests
    request_timeout: Duration,
    /// Retry Count for ARP Requests
    retry_count: usize,
    /// Initial Values for ARP Cache
    initial_values: HashMap<Ipv4Addr, MacAddress>,
    /// Disable ARP?
    disable_arp: bool,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for ARP Configuration Descriptor
impl ArpConfig {
    /// Creates an ARP Configuration Descriptor.
    pub fn new(config: &Config) -> Result<Self, Fail> {
        if let Some(initial_values) = config.arp_table()? {
            Ok(Self {
                cache_ttl: config.arp_cache_ttl()?,
                request_timeout: config.arp_request_timeout()?,
                retry_count: config.arp_request_retries()?,
                initial_values,
                disable_arp: false,
            })
        } else {
            warn!("disabling arp");
            Ok(Self {
                cache_ttl: Duration::ZERO,
                request_timeout: Duration::ZERO,
                retry_count: 0,
                initial_values: HashMap::new(),
                disable_arp: true,
            })
        }
    }

    /// Gets the time to live for entries of the ARP Cache in the target [ArpConfig].
    pub fn get_cache_ttl(&self) -> Duration {
        self.cache_ttl
    }

    /// Gets the request timeout for ARP requests in the target [ArpConfig].
    pub fn get_request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// Gets the retry count for ARP requests in the target [ArpConfig].
    pub fn get_retry_count(&self) -> usize {
        self.retry_count
    }

    /// Gets the initial values for the ARP Cache in the target [ArpConfig].
    pub fn get_initial_values(&self) -> &HashMap<Ipv4Addr, MacAddress> {
        &self.initial_values
    }

    /// Gets the disable option of the ARP in the target [ArpConfig].
    pub fn get_disable_arp(&self) -> bool {
        self.disable_arp
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Default Trait Implementation for ARP Configuration Descriptor
impl Default for ArpConfig {
    /// Creates a ARP Configuration Descriptor with the default values.
    fn default() -> Self {
        ArpConfig {
            cache_ttl: Duration::from_secs(15),
            request_timeout: Duration::from_secs(20),
            retry_count: 5,
            initial_values: HashMap::new(),
            disable_arp: false,
        }
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::network::config::ArpConfig;
    use ::anyhow::Result;
    use ::std::{
        collections::HashMap,
        time::Duration,
    };

    /// Tests default instantiation for [UdpConfig].
    #[test]
    fn test_arp_config_default() -> Result<()> {
        let config: ArpConfig = ArpConfig::default();
        crate::ensure_eq!(config.get_cache_ttl(), Duration::from_secs(15));
        crate::ensure_eq!(config.get_request_timeout(), Duration::from_secs(20));
        crate::ensure_eq!(config.get_retry_count(), 5);
        crate::ensure_eq!(config.get_initial_values(), &HashMap::new());
        crate::ensure_eq!(config.get_disable_arp(), false);

        Ok(())
    }
}
