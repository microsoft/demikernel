// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(test)]
mod tests;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::hashttlcache::HashTtlCache,
    runtime::network::types::MacAddress,
};
use ::std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::{
        Duration,
        Instant,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

const DUMMY_MAC_ADDRESS: MacAddress = MacAddress::new([0; 6]);

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Debug)]
struct Record {
    link_addr: MacAddress,
}

///
/// # ARP Cache
/// - TODO: Allow multiple waiters for the same address
/// - TODO: Deregister waiters here when the receiver goes away.
/// - TODO: Implement eviction.
/// - TODO: Implement remove.
/// Cache for IPv4 Addresses. If set to None, then ARP is disabled.
pub struct ArpCache(Option<HashTtlCache<Ipv4Addr, Record>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ArpCache {
    /// Creates an ARP Cache.
    pub fn new(
        now: Instant,
        default_ttl: Option<Duration>,
        values: Option<&HashMap<Ipv4Addr, MacAddress>>,
        disable: bool,
    ) -> ArpCache {
        ArpCache(if disable {
            None
        } else {
            let mut cache: HashTtlCache<Ipv4Addr, Record> = HashTtlCache::<Ipv4Addr, Record>::new(now, default_ttl);
            if let Some(values) = values {
                for (&k, &v) in values {
                    if let Some(record) = cache.insert(k, Record { link_addr: v }) {
                        warn!(
                            "Inserting two cache entries with the same address: address={:?} first MAC={:?} second \
                             MAC={:?}",
                            k, record, v
                        );
                    }
                }
            };
            Some(cache)
        })
    }

    /// Caches an address resolution.
    pub fn insert(&mut self, ipv4_addr: Ipv4Addr, link_addr: MacAddress) -> Option<MacAddress> {
        if let Some(ref mut cache) = self.0 {
            let record = Record { link_addr };
            cache.insert(ipv4_addr, record).map(|r| r.link_addr)
        } else {
            None
        }
    }

    /// Gets the MAC address of given IPv4 address.
    pub fn get(&self, ipv4_addr: Ipv4Addr) -> Option<&MacAddress> {
        if let Some(ref cache) = self.0 {
            cache.get(&ipv4_addr).map(|r| &r.link_addr)
        } else {
            Some(&DUMMY_MAC_ADDRESS)
        }
    }

    /// Clears the ARP cache.
    #[allow(unused)]
    pub fn clear(&mut self) {
        if let Some(ref mut cache) = self.0 {
            cache.clear()
        };
    }

    // Exports address resolutions that are stored in the ARP cache.
    #[cfg(test)]
    pub fn export(&self) -> HashMap<Ipv4Addr, MacAddress> {
        let mut map: HashMap<Ipv4Addr, MacAddress> = HashMap::default();
        if let Some(ref cache) = self.0 {
            for (k, v) in cache.iter() {
                map.insert(*k, v.link_addr);
            }
        }
        map
    }

    #[cfg(test)]
    pub fn advance_clock(&mut self, now: Instant) {
        if let Some(ref mut cache) = self.0 {
            cache.advance_clock(now)
        }
    }
}
