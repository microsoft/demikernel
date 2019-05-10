use std::time::Duration;

pub struct Options {
    cache_ttl: Duration
}

impl Default for Options {
    fn default() -> Self {
        Options {
            cache_ttl:
        }
    }
}
