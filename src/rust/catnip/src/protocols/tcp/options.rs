use float_duration::FloatDuration;
use std::time::Duration;

const DEFAULT_HANDSHAKE_TIMEOUT_SECS: f64 = 3.0;
const DEFAULT_HANDSHAKE_RETRIES: usize = 5;

#[derive(Clone)]
pub struct TcpOptions {
    pub handshake_retries: Option<usize>,
    pub handshake_timeout: Option<FloatDuration>,
}

impl TcpOptions {
    pub fn handshake_retries(&self) -> usize {
        self.handshake_retries.unwrap_or(DEFAULT_HANDSHAKE_RETRIES)
    }

    pub fn handshake_timeout(&self) -> Duration {
        self.handshake_timeout
            .unwrap_or_else(|| {
                FloatDuration::seconds(DEFAULT_HANDSHAKE_TIMEOUT_SECS)
            })
            .to_std()
            .unwrap()
    }
}
