use float_duration::FloatDuration;

#[derive(Clone)]
pub struct ArpOptions {
    pub cache_ttl: Option<FloatDuration>,
    pub request_timeout: Option<FloatDuration>,
    pub retry_count: Option<u64>,
}
