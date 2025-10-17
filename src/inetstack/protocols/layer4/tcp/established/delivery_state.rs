#![allow(dead_code)]
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::{
    collections::{
        async_queue::{AsyncQueue, SharedAsyncQueue},
        async_value::SharedAsyncValue,
    },
    inetstack::protocols::layer4::tcp::{established::sender::UnackedSegment, SeqNumber},
    runtime::memory::DemiBuffer,
};

// Hard limit for unsent queue.
// TODO: Remove this.  We should limit the unsent queue by either having a (configurable) send buffer size (in bytes,
// not segments) and rejecting send requests that exceed that, or by limiting the user's send buffer allocations.
const UNSENT_QUEUE_CUTOFF: usize = 1024;

// Minimum size for unacknowledged queue. This number doesn't really matter very much, it just sets the initial size
// of the unacked queue, below which memory allocation is not required.
const MIN_UNACKED_QUEUE_SIZE_FRAMES: usize = 64;

// Minimum size for unsent queue. This number doesn't really matter very much, it just sets the initial size
// of the unacked queue, below which memory allocation is not required.
const MIN_UNSENT_QUEUE_SIZE_FRAMES: usize = 64;

// The subset of state vars in Sender that are part of delivery in TCP
pub struct SenderState {
    //
    // Send Sequence Space:
    //
    //                     |<-----------------send window size----------------->|
    //                     |                                                    |
    //                send_unacked               send_next         send_unacked + send window
    //                     v                         v                          v
    // ... ----------------|-------------------------|--------------------------|--------------------------------
    //       acknowledged  |      unacknowledged     |     allowed to send      |  future sequence number space
    //
    // Note: In RFC 793 terminology, send_unacked is SND.UNA, send_next is SND.NXT, and "send window" is SND.WND.
    //

    // Sequence Number of the oldest byte of unacknowledged sent data.  In RFC 793 terms, this is SND.UNA.
    send_unacked: SharedAsyncValue<SeqNumber>,

    // Queue of unacknowledged sent data.  RFC 793 calls this the "retransmission queue".
    unacked_queue: SharedAsyncQueue<UnackedSegment>,

    // Send timers
    // Current retransmission timer expiration time.
    // TODO: Consider storing this directly in the RtoCalculator.
    retransmit_deadline_time_secs: SharedAsyncValue<Option<Instant>>,

    // In RFC 793 terms, this is SND.NXT.
    pub send_next_seq_no: SharedAsyncValue<SeqNumber>,

    // Sequence number of next data to be pushed but not sent. When there is an open window, this is equivalent to
    // send_next_seq_no.
    unsent_next_seq_no: SeqNumber,

    // Sequence number of the FIN, after we should never allocate more sequence numbers.
    fin_seq_no: Option<SeqNumber>,

    // This is the send buffer (user data we do not yet have window to send). If the option is None, then it indicates
    // a FIN. This keeps us from having to allocate an empty Demibuffer to indicate FIN.
    unsent_queue: SharedAsyncQueue<DemiBuffer>,
}

impl SenderState {
    pub fn new(local_seq_no: SeqNumber) -> Self {
        Self {
            send_unacked: SharedAsyncValue::new(local_seq_no),
            unacked_queue: SharedAsyncQueue::with_capacity(MIN_UNACKED_QUEUE_SIZE_FRAMES),
            retransmit_deadline_time_secs: SharedAsyncValue::new(None),
            send_next_seq_no: SharedAsyncValue::new(local_seq_no),
            unsent_next_seq_no: local_seq_no,
            fin_seq_no: None,
            unsent_queue: SharedAsyncQueue::with_capacity(MIN_UNSENT_QUEUE_SIZE_FRAMES),
        }
    }
}

pub struct ReceiverState {
    //
    // Receive Sequence Space:
    //
    //                     |<---------------receive_buffer_size---------------->|
    //                     |                                                    |
    //                     |                         |<-----receive window----->|
    //                 read_next               receive_next       receive_next + receive window
    //                     v                         v                          v
    // ... ----------------|-------------------------|--------------------------|------------------------------
    //      read by user   |  received but not read  |    willing to receive    | future sequence number space
    //
    // Note: In RFC 793 terminology, receive_next is RCV.NXT, and "receive window" is RCV.WND.
    //

    // Sequence number of next byte of data in the unread queue.
    reader_next_seq_no: SeqNumber,

    // Sequence number of the next byte of data (or FIN) that we expect to receive.  In RFC 793 terms, this is RCV.NXT.
    pub receive_next_seq_no: SeqNumber,

    // Sequnce number of the last byte of data (FIN).
    fin_seq_no: SharedAsyncValue<Option<SeqNumber>>,

    // Pop queue.  Contains in-order received (and acknowledged) data ready for the application to read.
    pop_queue: AsyncQueue<DemiBuffer>,

    // The amount of time before we will send a bare ACK.
    ack_delay_timeout_secs: Duration,
    // The deadline when we will send a bare ACK if there are no outgoing packets by then.
    pub ack_deadline_time_secs: SharedAsyncValue<Option<Instant>>,

    // This is our receive buffer size, which is also the maximum size of our receive window.
    // Note: The maximum possible advertised window is 1 GiB with window scaling and 64 KiB without.
    buffer_size_bytes: u32,

    // This is the number of bits to shift to convert to/from the scaled value, and has a maximum value of 14.
    window_scale_shift_bits: u8,

    // Queue of out-of-order segments.  This is where we hold onto data that we've received (because it was within our
    // receive window) but can't yet present to the user because we're missing some other data that comes between this
    // and what we've already presented to the user.
    out_of_order_frames: VecDeque<(SeqNumber, DemiBuffer)>,
}

impl ReceiverState {
    pub fn new(
        reader_next_seq_no: SeqNumber,
        receive_next_seq_no: SeqNumber,
        ack_delay_timeout_secs: Duration,
        window_size_bytes: u32,
        window_scale_shift_bits: u8,
    ) -> Self {
        Self {
            reader_next_seq_no,
            receive_next_seq_no,
            fin_seq_no: SharedAsyncValue::new(None),
            pop_queue: AsyncQueue::with_capacity(1024),
            ack_delay_timeout_secs,
            ack_deadline_time_secs: SharedAsyncValue::new(None),
            buffer_size_bytes: window_size_bytes,
            window_scale_shift_bits,
            out_of_order_frames: VecDeque::with_capacity(64),
        }
    }
}

pub struct DeliveryState {
    pub sender: SenderState,
    pub receiver: ReceiverState,
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
            sender: SenderState::new(local_seq_no),
            receiver: ReceiverState::new(
                reader_next_seq_no,
                receive_next_seq_no,
                ack_delay_timeout_secs,
                window_size_bytes,
                window_scale_shift_bits,
            ),
        }
    }
}
