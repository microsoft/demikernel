use float_duration::FloatDuration;

#[derive(Clone)]
pub struct ArpCacheOptions {
    pub default_ttl: Option<FloatDuration>,
}

impl Default for ArpCacheOptions {
    fn default() -> Self {
        ArpCacheOptions {
            // todo: need citation for default value.
            default_ttl: Some(FloatDuration::seconds(20.0)),
        }
    }
}
