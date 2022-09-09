// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::network::types::MacAddress;
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
    pub fn new(
        cache_ttl: Option<Duration>,
        request_timeout: Option<Duration>,
        retry_count: Option<usize>,
        initial_values: Option<HashMap<Ipv4Addr, MacAddress>>,
        disable_arp: Option<bool>,
    ) -> Self {
        let mut config: ArpConfig = Self::default();

        if let Some(cache_ttl) = cache_ttl {
            config.set_cache_ttl(cache_ttl);
        }
        if let Some(request_timeout) = request_timeout {
            config.set_request_timeout(request_timeout);
        }
        if let Some(retry_count) = retry_count {
            config.set_retry_count(retry_count);
        }
        if let Some(initial_values) = initial_values {
            config.set_initial_values(initial_values);
        }
        if let Some(disable_arp) = disable_arp {
            config.set_disable_arp(disable_arp);
        }

        config
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

    /// Sets the time to live for entries of the ARP Cache in the target [ArpConfig].
    fn set_cache_ttl(&mut self, cache_ttl: Duration) {
        self.cache_ttl = cache_ttl
    }

    /// Sets the request timeout for ARP requests in the target [ArpConfig].
    fn set_request_timeout(&mut self, request_timeout: Duration) {
        self.request_timeout = request_timeout
    }

    /// Sets the retry count for ARP requests in the target [ArpConfig].
    fn set_retry_count(&mut self, retry_count: usize) {
        self.retry_count = retry_count
    }

    /// Sets the initial values for the ARP Cache in the target [ArpConfig].
    fn set_initial_values(&mut self, initial_values: HashMap<Ipv4Addr, MacAddress>) {
        self.initial_values = initial_values;
    }

    /// Sets the disable option of the ARP in the target [ArpConfig].
    fn set_disable_arp(&mut self, disable_arp: bool) {
        self.disable_arp = disable_arp
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
    use ::std::{
        collections::HashMap,
        time::Duration,
    };

    /// Tests default instantiation for [UdpConfig].
    #[test]
    fn test_arp_config_default() {
        let config: ArpConfig = ArpConfig::default();
        assert_eq!(config.get_cache_ttl(), Duration::from_secs(15));
        assert_eq!(config.get_request_timeout(), Duration::from_secs(20));
        assert_eq!(config.get_retry_count(), 5);
        assert_eq!(config.get_initial_values(), &HashMap::new());
        assert_eq!(config.get_disable_arp(), false);
    }
}
