use crate::prelude::*;
use std::time::Instant;
use crate::protocols::ethernet2::EtherType;

use super::cache::ArpCache;
use super::options::ArpOptions;

pub struct ArpState {
    cache: ArpCache,
}

impl ArpState {
    pub fn from_options(options: &ArpOptions, now: Instant) -> ArpState {
        ArpState {
            cache: ArpCache::from_options(&options.cache, now),
        }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.cache.advance_clock(now)
    }

    pub fn receive(&mut self, packet: Packet) -> Result<Vec<Effect>> {
        let ether2_header = packet.parse_ether2_header()?;
        assert_eq!(EtherType::Arp as u16, ether2_header.ether_type);

        Ok(vec![])
    }
}
