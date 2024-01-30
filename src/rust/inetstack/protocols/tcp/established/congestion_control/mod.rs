// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod cubic;
mod none;
mod options;

use crate::{
    collections::async_value::SharedAsyncValue,
    inetstack::protocols::tcp::SeqNumber,
};
use ::std::{
    fmt::Debug,
    time::Duration,
};

pub use self::{
    cubic::Cubic,
    none::None,
    options::{
        OptionValue,
        Options,
    },
};

pub trait SlowStartCongestionAvoidance {
    fn get_cwnd(&self) -> SharedAsyncValue<u32>;

    // Called immediately before the cwnd check is performed before data is sent.
    fn on_cwnd_check_before_send(&mut self) {}

    fn on_ack_received(
        &mut self,
        _rto: Duration,
        _send_unacked: SeqNumber,
        _send_next: SeqNumber,
        _ack_seq_no: SeqNumber,
    ) {
    }

    // Called immediately before retransmit after RTO.
    fn on_rto(&mut self, _send_unacked: SeqNumber) {}

    // Called immediately before a segment is sent for the 1st time.
    fn on_send(&mut self, _rto: Duration, _num_sent_bytes: u32) {}
}

pub trait FastRetransmitRecovery
where
    Self: SlowStartCongestionAvoidance,
{
    fn get_duplicate_ack_count(&self) -> u32 {
        0
    }

    fn get_retransmit_now_flag(&self) -> SharedAsyncValue<bool>;

    fn on_fast_retransmit(&mut self) {}
}

pub trait LimitedTransmit
where
    Self: SlowStartCongestionAvoidance,
{
    fn get_limited_transmit_cwnd_increase(&self) -> SharedAsyncValue<u32>;
}

pub trait CongestionControl: SlowStartCongestionAvoidance + FastRetransmitRecovery + LimitedTransmit + Debug {
    fn new(mss: usize, seq_no: SeqNumber, options: Option<options::Options>) -> Box<dyn CongestionControl>
    where
        Self: Sized;
}

pub type CongestionControlConstructor = fn(usize, SeqNumber, Option<options::Options>) -> Box<dyn CongestionControl>;
