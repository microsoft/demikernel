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
    collections::async_value::SharedAsyncValue,
    inetstack::protocols::tcp::SeqNumber,
};
use ::std::fmt::Debug;

// Implementation of congestion control which does nothing.
#[derive(Debug)]
pub struct None {
    cwnd: SharedAsyncValue<u32>,
    fast_retransmit_flag: SharedAsyncValue<bool>,
    limited_retransmit_cwnd_increase: SharedAsyncValue<u32>,
}

impl CongestionControl for None {
    fn new(_mss: usize, _seq_no: SeqNumber, _options: Option<Options>) -> Box<dyn CongestionControl> {
        Box::new(Self {
            cwnd: SharedAsyncValue::new(u32::MAX),
            fast_retransmit_flag: SharedAsyncValue::new(false),
            limited_retransmit_cwnd_increase: SharedAsyncValue::new(0),
        })
    }
}

impl SlowStartCongestionAvoidance for None {
    fn get_cwnd(&self) -> SharedAsyncValue<u32> {
        self.cwnd.clone()
    }
}
impl FastRetransmitRecovery for None {
    fn get_retransmit_now_flag(&self) -> SharedAsyncValue<bool> {
        self.fast_retransmit_flag.clone()
    }
}
impl LimitedTransmit for None {
    fn get_limited_transmit_cwnd_increase(&self) -> SharedAsyncValue<u32> {
        self.limited_retransmit_cwnd_increase.clone()
    }
}
