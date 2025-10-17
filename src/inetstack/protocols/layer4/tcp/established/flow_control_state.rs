#![allow(dead_code)]
use crate::{collections::async_value::SharedAsyncValue, inetstack::protocols::layer4::tcp::SeqNumber};

pub struct FlowControlState {
    send_window: SharedAsyncValue<u32>,
    send_window_scale_shift_bits: u8,
    send_window_last_update_seq: SeqNumber, // SND.WL1
    send_window_last_update_ack: SeqNumber, // SND.WL2
    mss: usize,
}

impl FlowControlState {
    pub fn new(
        local_seq_no: SeqNumber,
        remote_seq_no: SeqNumber,
        send_window: u32,
        send_window_scale_shift_bits: u8,
        mss: usize,
    ) -> Self {
        Self {
            send_window: SharedAsyncValue::new(send_window),
            send_window_scale_shift_bits,
            send_window_last_update_seq: remote_seq_no,
            send_window_last_update_ack: local_seq_no,
            mss,
        }
    }
}
