use super::ArpCache;
use std::time::{Duration, Instant};

pub struct ArpCacheOptions {
    ttl: Option<Duration>,
}

impl Default for ArpCacheOptions {
    fn default() -> Self {
        ArpCacheOptions {
            // todo: need citation for default value.
            ttl: Some(Duration::from_secs(20)),
        }
    }
}

impl ArpCacheOptions {
    pub fn new_cache(self, now: Instant) -> ArpCache {
        ArpCache::new(self.ttl, now)
    }
}
