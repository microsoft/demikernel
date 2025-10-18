#![allow(dead_code)]
use std::time::Duration;

use crate::inetstack::protocols::layer4::tcp::{
    established::{receiver::Receiver, sender::Sender},
    SeqNumber,
};

pub struct DeliveryState {
    pub sender: Sender,
    pub receiver: Receiver,
}

impl DeliveryState {
    pub fn new(
        local_seq_no: SeqNumber,
        reader_next_seq_no: SeqNumber,
        receive_next_seq_no: SeqNumber,
        ack_delay_timeout_secs: Duration,
        window_size_bytes: u32,
        window_scale_shift_bits: u8,
    ) -> Self {
        Self {
            sender: Sender::new(local_seq_no),
            receiver: Receiver::new(
                reader_next_seq_no,
                receive_next_seq_no,
                ack_delay_timeout_secs,
                window_size_bytes,
                window_scale_shift_bits,
            ),
        }
    }
}
