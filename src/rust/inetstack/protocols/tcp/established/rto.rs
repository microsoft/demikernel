// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::time::Duration;

// TCP Retransmission Timeout (RTO) Calculator.
// See RFC 6298 for details.

// ToDo: Issue #371 Consider reimplementing this using integer arithmetic instead of floating-point.

#[derive(Debug)]
pub struct RtoCalculator {
    // Smoothed round-trip time.
    srtt: f64,

    // Round-trip time variation.
    rttvar: f64,

    // Retransmission timeout.
    rto: f64,

    // Whether a RTT (round-trip-time) sample has been received yet.
    received_sample: bool,
}

impl RtoCalculator {
    /// Initializes an RTO Calculator.
    pub fn new() -> Self {
        // RFC 6298 recommends an initial value of 1 second for RTO (See also RFC 6298 Appendix A).  The initial values
        // for SRTT and RTTVAR are arbitrary as they aren't used until after the first sample has been received.
        Self {
            srtt: 1.0,
            rttvar: 0.0,
            rto: 1.0,
            received_sample: false,
        }
    }

    /// Adds an RTT sample to the calculator.
    pub fn add_sample(&mut self, rtt: Duration) {
        // RFC 6298's suggested value for alpha is 1/8.
        const ALPHA: f64 = 0.125;
        // RFC 6298's suggested value for beta is 1/4.
        const BETA: f64 = 0.25;
        // Clock granularity in seconds.
        const GRANULARITY: f64 = 0.001f64;

        let rtt: f64 = rtt.as_secs_f64();

        if !self.received_sample {
            // Initial sample formula from RFC 6298 Section 2.2:
            self.srtt = rtt;
            self.rttvar = rtt / 2.;
            self.received_sample = true;
        } else {
            // Subsequent sample formula from RFC 6298 Section 2.3:
            self.rttvar = (1.0 - BETA) * self.rttvar + BETA * (self.srtt - rtt).abs();
            self.srtt = (1.0 - ALPHA) * self.srtt + ALPHA * rtt;
        }

        // The new RTO value is the smoothed RTT plus the maximum of the clock granularity and 4 times the RTT variance.
        let rto: f64 = self.srtt + GRANULARITY.max(4.0 * self.rttvar);

        // Store the updated RTT value.
        self.update_rto(rto);
    }

    /// Updates the stored RTO value while keeping it within the prescribed bounds (RFC 6298 Section 2.4)
    fn update_rto(&mut self, new_rto: f64) {
        // RFC 6298's suggested value for the lower bound is 1 second.  Note this currently uses 1/10 of a second.
        const LOWER_BOUND_SEC: f64 = 0.100f64;
        // RFC 6298's suggested value for the upper bound is >= 60 seconds.
        const UPPER_BOUND_SEC: f64 = 60.0f64;

        // Note: We use clamp() below as it is clearer in intent than a min/max combination.  However, if we were
        // concerned that new_rto could be NaN here (we're not) we wouldn't want to use clamp() as it would pass NaN
        // through.  We'd use "self.rto = f64::min(new_rto.max(LOWER_BOUND_SEC), UPPER_BOUND_SEC);" below instead.
        self.rto = new_rto.clamp(LOWER_BOUND_SEC, UPPER_BOUND_SEC);
    }

    /// Performs an exponential "back off" of the RTO (doubles the current timeout).
    pub fn back_off(&mut self) {
        self.update_rto(self.rto * 2.0);
    }

    /// Gets the current RTO value.
    pub fn rto(&self) -> Duration {
        Duration::from_secs_f64(self.rto)
    }
}
