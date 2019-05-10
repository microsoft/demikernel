use std::time::{Duration, Instant};
use super::ArpCache;

pub struct ArpCacheOptions {
    ttl: Duration
}

impl Default for ArpCacheOptions {
    fn default() -> Self {
        ArpCacheOptions {
            // todo: need citation for default value.
            ttl: Duration::from_secs(20),
        }
    }
}

impl ArpCacheOptions {
    pub fn new_cache(self, now: Instant) -> ArpCache {
        ArpCache::new(self.ttl, now)
    }
}
