// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    CongestionControl,
    FastRetransmitRecovery,
    LimitedTransmit,
    Options,
    SlowStartCongestionAvoidance,
};
use crate::{
    inetstack::protocols::tcp::SeqNumber,
    runtime::watched::SharedWatchedValue,
};
use ::std::fmt::Debug;

// Implementation of congestion control which does nothing.
#[derive(Debug)]
pub struct None {
    cwnd: SharedWatchedValue<u32>,
    fast_retransmit_flag: SharedWatchedValue<bool>,
    limited_retransmit_cwnd_increase: SharedWatchedValue<u32>,
}

impl CongestionControl for None {
    fn new(_mss: usize, _seq_no: SeqNumber, _options: Option<Options>) -> Box<dyn CongestionControl> {
        Box::new(Self {
            cwnd: SharedWatchedValue::new(u32::MAX),
            fast_retransmit_flag: SharedWatchedValue::new(false),
            limited_retransmit_cwnd_increase: SharedWatchedValue::new(0),
        })
    }
}

impl SlowStartCongestionAvoidance for None {
    fn get_cwnd(&self) -> SharedWatchedValue<u32> {
        self.cwnd.clone()
    }
}
impl FastRetransmitRecovery for None {
    fn get_retransmit_now_flag(&self) -> SharedWatchedValue<bool> {
        self.fast_retransmit_flag.clone()
    }
}
impl LimitedTransmit for None {
    fn get_limited_transmit_cwnd_increase(&self) -> SharedWatchedValue<u32> {
        self.limited_retransmit_cwnd_increase.clone()
    }
}
