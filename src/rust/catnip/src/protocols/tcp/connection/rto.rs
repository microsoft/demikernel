// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use float_duration::FloatDuration;
use std::time::Duration;

// this is an implementation of the "classic" RTO computation method as described in [TCP/IP Illustrated](https://learning.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/ch14.html)

const ALPHA: f64 = (0.8f64 + 0.9) / 2.0;
const BETA: f64 = (1.3f64 + 2.0) / 2.0;
const UBOUND_SEC: f64 = 60.0f64;
// we use a smaller lower bound than is recommended because of the low latency
// network hardware.
const LBOUND_SEC: f64 = 0.0001f64;
const SRTT_SEED: f64 = 1.0f64;

#[derive(Debug)]
pub struct RtoCalculator {
    srtt: f64,
}

impl RtoCalculator {
    pub fn new() -> Self {
        RtoCalculator { srtt: SRTT_SEED }
    }

    pub fn add_sample(&mut self, rtt: Duration) {
        let rtt = FloatDuration::from(rtt).as_seconds();
        self.srtt = ALPHA * self.srtt + (1.0 - ALPHA) * rtt;
    }

    pub fn rto(&self) -> Duration {
        let secs = UBOUND_SEC.min(LBOUND_SEC.max(self.srtt * BETA));
        FloatDuration::seconds(secs).to_std().unwrap()
    }
}
