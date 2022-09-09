// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(test)]
mod tests;

use crate::{
    inetstack::collections::HashTtlCache,
    runtime::{
        network::types::MacAddress,
        timer::TimerRc,
    },
};
use ::std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Constants
//==============================================================================

const DUMMY_MAC_ADDRESS: MacAddress = MacAddress::new([0; 6]);

//==============================================================================
// Structures
//==============================================================================

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
pub struct ArpCache {
    /// Cache for IPv4 Addresses
    cache: HashTtlCache<Ipv4Addr, Record>,

    /// Disable ARP?
    disable: bool,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl ArpCache {
    /// Creates an ARP Cache.
    pub fn new(
        clock: TimerRc,
        default_ttl: Option<Duration>,
        values: Option<&HashMap<Ipv4Addr, MacAddress>>,
        disable: bool,
    ) -> ArpCache {
        let mut peer = ArpCache {
            cache: HashTtlCache::new(clock.now(), default_ttl),
            disable,
        };

        // Populate cache.
        if let Some(values) = values {
            for (&k, &v) in values {
                peer.insert(k, v);
            }
        }

        peer
    }

    /// Caches an address resolution.
    pub fn insert(&mut self, ipv4_addr: Ipv4Addr, link_addr: MacAddress) -> Option<MacAddress> {
        let record = Record { link_addr };
        self.cache.insert(ipv4_addr, record).map(|r| r.link_addr)
    }

    /// Gets the MAC address of given IPv4 address.
    pub fn get(&self, ipv4_addr: Ipv4Addr) -> Option<&MacAddress> {
        if self.disable {
            Some(&DUMMY_MAC_ADDRESS)
        } else {
            self.cache.get(&ipv4_addr).map(|r| &r.link_addr)
        }
    }

    /// Advances internal clock of the ARP Cache.
    pub fn advance_clock(&mut self, now: Instant) {
        self.cache.advance_clock(now)
    }

    /// Clears the ARP cache.
    #[allow(unused)]
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    // Exports address resolutions that are stored in the ARP cache.
    #[cfg(test)]
    pub fn export(&self) -> HashMap<Ipv4Addr, MacAddress> {
        let mut map: HashMap<Ipv4Addr, MacAddress> = HashMap::default();
        for (k, v) in self.cache.iter() {
            map.insert(*k, v.link_addr);
        }
        map
    }
}
