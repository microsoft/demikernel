use crate::protocols::tcp2::SeqNumber;
use std::convert::TryInto;
use crate::collections::watched::WatchedValue;
use std::collections::VecDeque;
use crate::fail::Fail;
use std::num::Wrapping;
use std::cell::{RefCell};
use std::time::{Instant};
use super::rto::RtoCalculator;

pub struct UnackedSegment {
    pub bytes: Vec<u8>,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SenderState {
    Open,
    Closed,
    SentFin,
    FinAckd,

    #[allow(unused)]
    Reset,
}

pub struct Sender {
    pub state: WatchedValue<SenderState>,

    // TODO: Just use Figure 5 from RFC 793 here.
    //
    //                    |------------window_size------------|
    //
    //               base_seq_no               sent_seq_no           unsent_seq_no
    //                    v                         v                      v
    // ... ---------------|-------------------------|----------------------| (unavailable)
    //       acknowleged         unacknowledged     ^        unsent
    //
    pub base_seq_no: WatchedValue<SeqNumber>,
    pub unacked_queue: RefCell<VecDeque<UnackedSegment>>,
    pub sent_seq_no: WatchedValue<SeqNumber>,
    pub unsent_queue: RefCell<VecDeque<Vec<u8>>>,
    pub unsent_seq_no: WatchedValue<SeqNumber>,

    pub window_size: WatchedValue<u32>,
    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    pub window_scale: u8,

    pub mss: usize,

    pub retransmit_deadline: WatchedValue<Option<Instant>>,
    pub rto: RefCell<RtoCalculator>,
}

impl Sender {
    pub fn new(seq_no: SeqNumber, window_size: u32, window_scale: u8, mss: usize) -> Self {
        Self {
            state: WatchedValue::new(SenderState::Open),

            base_seq_no: WatchedValue::new(seq_no),
            unacked_queue: RefCell::new(VecDeque::new()),
            sent_seq_no: WatchedValue::new(seq_no),
            unsent_queue: RefCell::new(VecDeque::new()),
            unsent_seq_no: WatchedValue::new(seq_no),

            window_size: WatchedValue::new(window_size),
            window_scale,
            mss,

            retransmit_deadline: WatchedValue::new(None),
            rto: RefCell::new(RtoCalculator::new()),
        }
    }

    pub fn send(&self, buf: Vec<u8>) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored { details: "Sender closed" });
        }
        let buf_len: u32 = buf.len().try_into()
            .map_err(|_| Fail::Ignored { details: "Buffer too large" })?;
        self.unsent_queue.borrow_mut().push_back(buf);
        self.unsent_seq_no.modify(|s| s + Wrapping(buf_len));
        Ok(())
    }

    pub fn close(&self) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored { details: "Sender closed" });
        }
        self.state.set(SenderState::Closed);
        Ok(())
    }

    pub fn remote_ack(&self, ack_seq_no: SeqNumber, now: Instant) -> Result<(), Fail> {
        if self.state.get() == SenderState::SentFin {
            assert_eq!(self.base_seq_no.get(), self.sent_seq_no.get());
            assert_eq!(self.sent_seq_no.get(), self.unsent_seq_no.get());
            self.state.set(SenderState::FinAckd);
            return Ok(());
        }

        let base_seq_no = self.base_seq_no.get();
        let sent_seq_no = self.sent_seq_no.get();

        let bytes_outstanding = sent_seq_no - base_seq_no;
        let bytes_acknowledged = ack_seq_no - base_seq_no;

        if bytes_acknowledged > bytes_outstanding {
            return Err(Fail::Ignored { details: "ACK is outside of send window" });
        }
        if bytes_acknowledged.0 == 0 {
            // TODO: Handle fast retransmit here.
            return Ok(());
        }

        if ack_seq_no == sent_seq_no {
            // If we've acknowledged all sent data, turn off the retransmit timer.
            self.retransmit_deadline.set(None);
        } else {
            // Otherwise, set it to the current RTO.
            let deadline = now + self.rto.borrow().estimate();
            self.retransmit_deadline.set(Some(deadline));
        }

        // TODO: Do acks need to be on segment boundaries? How does this interact with repacketization?
        let mut bytes_remaining = bytes_acknowledged.0 as usize;
        while let Some(segment) = self.unacked_queue.borrow_mut().pop_front() {
            if segment.bytes.len() > bytes_remaining {
                // TODO: We need to close the connection in this case.
                return Err(Fail::Ignored { details: "ACK isn't on segment boundary" });
            }
            bytes_remaining -= segment.bytes.len();

            // Add sample for RTO if not a retransmission
            // TODO: TCP timestamp support.
            if let Some(initial_tx) = segment.initial_tx {
                self.rto.borrow_mut().add_sample(now - initial_tx);
            }
            if bytes_remaining == 0 {
                break;
            }
        }
        self.base_seq_no.modify(|b| b + bytes_acknowledged);

        Ok(())
    }

    pub fn pop_one_unsent_byte(&self) -> Option<Vec<u8>> {
        let mut queue = self.unsent_queue.borrow_mut();
        let mut buf = queue.pop_front()?;
        let remainder = buf.split_off(1);
        queue.push_front(remainder);
        Some(buf)
    }

    pub fn pop_unsent(&self, mut bytes_remaining: usize) -> Vec<u8> {
        let mut segment_data = vec![];
        let mut unsent_queue = self.unsent_queue.borrow_mut();
        while bytes_remaining > 0 {
            let mut buf = match unsent_queue.pop_front() {
                Some(b) => b,
                None => break,
            };
            if buf.len() > bytes_remaining {
                let tail = buf.split_off(bytes_remaining);
                unsent_queue.push_front(tail);
            }
            bytes_remaining -= buf.len();
            segment_data.extend(buf);
        }
        segment_data
    }

    pub fn update_remote_window(&self, window_size_hdr: u16) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored { details: "Dropping remote window update for closed sender" });
        }

        // TODO: Is this the right check?
        let window_size = (window_size_hdr as u32).checked_shl(self.window_scale as u32)
            .ok_or_else(|| Fail::Ignored { details: "Window size overflow" })?;
        self.window_size.set(window_size);

        Ok(())
    }
}
