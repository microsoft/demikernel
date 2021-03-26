use float_duration::FloatDuration;
use std::{
    cmp,
    time::Duration,
};

// RFC6298
#[derive(Debug)]
pub struct RtoCalculator {
    srtt: f64,
    rttvar: f64,
    rto: f64,

    received_sample: bool,
}

impl RtoCalculator {
    pub fn new() -> Self {
        Self {
            srtt: 1.0,
            rttvar: 0.0,
            rto: 1.0,

            received_sample: false,
        }
    }

    pub fn add_sample(&mut self, rtt: Duration) {
        const ALPHA: f64 = 0.125;
        const BETA: f64 = 0.25;
        const GRANULARITY: f64 = 0.001f64;

        let rtt = FloatDuration::from(rtt).as_seconds();

        if !self.received_sample {
            self.srtt = rtt;
            self.rttvar = rtt / 2.;
            self.received_sample = true;
        } else {
            self.rttvar = (1.0 - BETA) * self.rttvar + BETA * (self.srtt - rtt).abs();
            self.srtt = (1.0 - ALPHA) * self.srtt + ALPHA * rtt;
        }

        let rttvar_x4 = match (4.0 * self.rttvar).partial_cmp(&GRANULARITY) {
            Some(cmp::Ordering::Less) => GRANULARITY,
            None => panic!("NaN rttvar: {:?}", self.rttvar),
            _ => self.rttvar,
        };
        self.update_rto(self.srtt + rttvar_x4);
    }

    fn update_rto(&mut self, new_rto: f64) {
        const UBOUND_SEC: f64 = 60.0f64;
        const LBOUND_SEC: f64 = 0.100f64;
        self.rto = match (
            new_rto.partial_cmp(&LBOUND_SEC),
            new_rto.partial_cmp(&UBOUND_SEC),
        ) {
            (Some(cmp::Ordering::Less), _) => LBOUND_SEC,
            (_, Some(cmp::Ordering::Greater)) => UBOUND_SEC,
            (None, _) | (_, None) => panic!("NaN RTO: {:?}", new_rto),
            _ => new_rto,
        };
    }

    pub fn record_failure(&mut self) {
        self.update_rto(self.rto * 2.0);
    }

    pub fn estimate(&self) -> Duration {
        FloatDuration::seconds(self.rto).to_std().unwrap()
    }
}
