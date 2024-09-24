// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::{fail::Fail, network::types::MacAddress},
};
use ::std::{collections::HashMap, net::Ipv4Addr, time::Duration};

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Debug)]
pub struct ArpConfig {
    cache_ttl: Duration,
    request_timeout: Duration,
    retry_count: usize,
    initial_values: HashMap<Ipv4Addr, MacAddress>,
    is_enabled: bool,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ArpConfig {
    pub fn new(config: &Config) -> Result<Self, Fail> {
        if let Some(initial_values) = config.arp_table()? {
            Ok(Self {
                cache_ttl: config.arp_cache_ttl()?,
                request_timeout: config.arp_request_timeout()?,
                retry_count: config.arp_request_retries()?,
                initial_values,
                is_enabled: true,
            })
        } else {
            warn!("disabling arp");
            Ok(Self {
                cache_ttl: Duration::ZERO,
                request_timeout: Duration::ZERO,
                retry_count: 0,
                initial_values: HashMap::new(),
                is_enabled: false,
            })
        }
    }

    pub fn get_cache_ttl(&self) -> Duration {
        self.cache_ttl
    }

    pub fn get_request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn get_retry_count(&self) -> usize {
        self.retry_count
    }

    pub fn get_initial_values(&self) -> &HashMap<Ipv4Addr, MacAddress> {
        &self.initial_values
    }

    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for ArpConfig {
    fn default() -> Self {
        ArpConfig {
            cache_ttl: Duration::from_secs(15),
            request_timeout: Duration::from_secs(20),
            retry_count: 5,
            initial_values: HashMap::new(),
            is_enabled: true,
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::network::config::ArpConfig;
    use ::anyhow::Result;
    use ::std::{collections::HashMap, time::Duration};

    #[test]
    fn test_arp_config_default() -> Result<()> {
        let config: ArpConfig = ArpConfig::default();
        crate::ensure_eq!(config.get_cache_ttl(), Duration::from_secs(15));
        crate::ensure_eq!(config.get_request_timeout(), Duration::from_secs(20));
        crate::ensure_eq!(config.get_retry_count(), 5);
        crate::ensure_eq!(config.get_initial_values(), &HashMap::new());
        crate::ensure_eq!(config.is_enabled(), true);

        Ok(())
    }
}
