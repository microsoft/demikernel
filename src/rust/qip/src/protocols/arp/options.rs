use super::cache::ArpCacheOptions;
use float_duration::FloatDuration;

#[derive(Clone)]
pub struct ArpOptions {
    pub cache: ArpCacheOptions,
    pub request_timeout: Option<FloatDuration>,
    pub retry_count: Option<u64>,
}
