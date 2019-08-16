use float_duration::FloatDuration;
use std::time::Duration;

const DEFAULT_CACHE_TTL_SECS: f64 = 15.0;
const DEFAULT_REQUEST_TIMEOUT_SECS: f64 = 20.0;
const DEFAULT_RETRY_COUNT: usize = 5;

#[derive(Clone, Debug)]
pub struct ArpOptions {
    pub cache_ttl: Option<FloatDuration>,
    pub request_timeout: Option<FloatDuration>,
    pub retry_count: Option<usize>,
}

impl ArpOptions {
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
            .unwrap_or_else(|| {
                FloatDuration::seconds(DEFAULT_REQUEST_TIMEOUT_SECS)
            })
            .to_std()
            .unwrap()
    }

    pub fn retry_count(&self) -> usize {
        self.retry_count.unwrap_or(DEFAULT_RETRY_COUNT)
    }

    pub fn cache_ttl(&self) -> Duration {
        self.cache_ttl
            .unwrap_or_else(|| FloatDuration::seconds(DEFAULT_CACHE_TTL_SECS))
            .to_std()
            .unwrap()
    }
}
