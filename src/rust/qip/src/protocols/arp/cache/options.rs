use std::time::Duration;

#[derive(Clone)]
pub struct ArpCacheOptions {
    pub default_ttl: Option<Duration>,
}

impl Default for ArpCacheOptions {
    fn default() -> Self {
        ArpCacheOptions {
            // todo: need citation for default value.
            default_ttl: Some(Duration::from_secs(20)),
        }
    }
}
