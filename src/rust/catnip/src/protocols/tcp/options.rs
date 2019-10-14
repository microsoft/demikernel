use super::segment::{DEFAULT_MSS, MAX_MSS, MIN_MSS};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TcpOptions {
    pub advertised_mss: usize,
    pub handshake_retries: usize,
    pub handshake_timeout: Duration,
    pub receive_window_size: usize,
    pub retries2: usize,
    pub trailing_ack_delay: Duration,
}

impl Default for TcpOptions {
    fn default() -> Self {
        TcpOptions {
            advertised_mss: DEFAULT_MSS,
            handshake_retries: 5,
            handshake_timeout: Duration::from_secs(3),
            receive_window_size: 0xffff,
            retries2: 5,
            trailing_ack_delay: Duration::from_micros(100),
        }
    }
}

impl TcpOptions {
    pub fn advertised_mss(mut self, value: usize) -> Self {
        assert!(value >= MIN_MSS);
        assert!(value <= MAX_MSS);
        self.advertised_mss = value;
        self
    }

    pub fn handshake_retries(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.handshake_retries = value;
        self
    }

    pub fn handshake_timeout(mut self, value: Duration) -> Self {
        assert!(value > Duration::new(0, 0));
        self.handshake_timeout = value;
        self
    }

    pub fn receive_window_size(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.receive_window_size = value;
        self
    }

    pub fn retries2(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.retries2 = value;
        self
    }

    pub fn trailing_ack_delay(mut self, value: Duration) -> Self {
        self.trailing_ack_delay = value;
        self
    }
}
