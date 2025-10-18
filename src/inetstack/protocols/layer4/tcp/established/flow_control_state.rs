use std::fmt;

use crate::{
    collections::async_value::SharedAsyncValue,
    inetstack::protocols::layer4::tcp::{header::TcpHeader, SeqNumber},
};

pub struct FlowControlState {
    // Available window to send into, as advertised by our peer.  In RFC 793 terms, this is SND.WND.
    pub send_window: SharedAsyncValue<u32>,
    pub send_window_last_update_seq: SeqNumber, // SND.WL1
    pub send_window_last_update_ack: SeqNumber, // SND.WL2

    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    pub send_window_scale_shift_bits: u8,

    // Maximum Segment Size currently in use for this connection.
    // TODO: Revisit this once we support path MTU discovery.
    pub mss: usize,
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

    pub fn update_send_window(&mut self, header: &TcpHeader) {
        // Make sure the ack num is bigger than the last one that we used to update the send window.
        if self.send_window_last_update_seq < header.seq_num
            || (self.send_window_last_update_seq == header.seq_num
                && self.send_window_last_update_ack <= header.ack_num)
        {
            self.send_window
                .set((header.window_size as u32) << self.send_window_scale_shift_bits);
            self.send_window_last_update_seq = header.seq_num;
            self.send_window_last_update_ack = header.ack_num;

            debug!(
                "Updating window size -> {} (hdr {}, scale {})",
                self.send_window.get(),
                header.window_size,
                self.send_window_scale_shift_bits,
            );
        }
    }
}

impl fmt::Debug for FlowControlState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FlowControlState")
            .field("send_window", &self.send_window)
            .field("window_scale", &self.send_window_scale_shift_bits)
            .field("mss", &self.mss)
            .finish()
    }
}
