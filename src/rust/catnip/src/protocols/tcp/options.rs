use super::segment::DEFAULT_MSS;
use float_duration::FloatDuration;
use std::time::Duration;

const DEFAULT_HANDSHAKE_TIMEOUT_SECS: f64 = 3.0;
const DEFAULT_HANDSHAKE_RETRIES: usize = 5;
const DEFAULT_RECEIVE_WINDOW_SIZE: usize = 0xffff;
const DEFAULT_TRAILING_ACK_DELAY_SECS: f64 = 0.000_1;
const DEFAULT_RETRIES2: usize = 5;

#[derive(Clone, Debug, Default)]
pub struct TcpOptions {
    pub advertised_mss: Option<usize>,
    pub handshake_retries: Option<usize>,
    pub handshake_timeout: Option<FloatDuration>,
    pub receive_window_size: Option<usize>,
    pub retries2: Option<usize>,
    pub trailing_ack_delay: Option<FloatDuration>,
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

    pub fn receive_window_size(&self) -> usize {
        self.receive_window_size
            .unwrap_or(DEFAULT_RECEIVE_WINDOW_SIZE)
    }

    pub fn trailing_ack_delay(&self) -> Duration {
        self.trailing_ack_delay
            .unwrap_or_else(|| {
                FloatDuration::seconds(DEFAULT_TRAILING_ACK_DELAY_SECS)
            })
            .to_std()
            .unwrap()
    }

    pub fn retries2(&self) -> usize {
        self.retries2.unwrap_or(DEFAULT_RETRIES2)
    }

    pub fn advertised_mss(&self) -> usize {
        self.advertised_mss.unwrap_or(DEFAULT_MSS)
    }
}
