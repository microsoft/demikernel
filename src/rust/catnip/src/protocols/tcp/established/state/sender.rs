use super::rto::RtoCalculator;
use crate::{
    collections::watched::WatchedValue,
    fail::Fail,
    protocols::tcp::SeqNumber,
    runtime::{Runtime, RuntimeBuf},
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::TryInto,
    fmt,
    num::Wrapping,
    time::{
        Duration,
        Instant,
    },
};

pub struct UnackedSegment<RT: Runtime> {
    pub bytes: RT::Buf,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SenderState {
    Open,
    Closed,
    SentFin,
    FinAckd,
    Reset,
}

pub struct Sender<RT: Runtime> {
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
    pub unacked_queue: RefCell<VecDeque<UnackedSegment<RT>>>,
    pub sent_seq_no: WatchedValue<SeqNumber>,
    pub unsent_queue: RefCell<VecDeque<RT::Buf>>,
    pub unsent_seq_no: WatchedValue<SeqNumber>,

    pub window_size: WatchedValue<u32>,
    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    pub window_scale: u8,

    pub mss: usize,

    pub retransmit_deadline: WatchedValue<Option<Instant>>,
    pub rto: RefCell<RtoCalculator>,
}

impl<RT: Runtime> fmt::Debug for Sender<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("base_seq_no", &self.base_seq_no)
            .field("sent_seq_no", &self.sent_seq_no)
            .field("unsent_seq_no", &self.unsent_seq_no)
            .field("window_size", &self.window_size)
            .field("window_scale", &self.window_scale)
            .field("mss", &self.mss)
            .field("retransmit_deadline", &self.retransmit_deadline)
            .field("rto", &self.rto)
            .finish()
    }
}

impl<RT: Runtime> Sender<RT> {
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

    pub fn send(&self, buf: RT::Buf, cb: &super::ControlBlock<RT>) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored {
                details: "Sender closed",
            });
        }
        let buf_len: u32 = buf.len().try_into().map_err(|_| Fail::Ignored {
            details: "Buffer too large",
        })?;

        let win_sz = self.window_size.get();
        let base_seq = self.base_seq_no.get();
        let sent_seq = self.sent_seq_no.get();
        let Wrapping(sent_data) = sent_seq - base_seq;

        // Fast path: Try to send the data immediately.
        if win_sz > 0 && win_sz >= sent_data + buf_len {
            if let Some(remote_link_addr) = cb.arp.try_query(cb.remote.address()) {
                let mut header = cb.tcp_header();
                header.seq_num = sent_seq;
                cb.emit(header, buf.clone(), remote_link_addr);

                self.unsent_seq_no.modify(|s| s + Wrapping(buf_len));
                self.sent_seq_no.modify(|s| s + Wrapping(buf_len));
                let unacked_segment = UnackedSegment {
                    bytes: buf,
                    initial_tx: Some(cb.rt.now()),
                };
                self.unacked_queue.borrow_mut().push_back(unacked_segment);
                if self.retransmit_deadline.get().is_none() {
                    let rto = self.rto.borrow().estimate();
                    self.retransmit_deadline.set(Some(cb.rt.now() + rto));
                }
                return Ok(());
            }
        }
        // Slow path: Delegating sending the data to background processing.
        self.unsent_queue.borrow_mut().push_back(buf);
        self.unsent_seq_no.modify(|s| s + Wrapping(buf_len));

        Ok(())
    }

    pub fn close(&self) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored {
                details: "Sender closed",
            });
        }
        self.state.set(SenderState::Closed);
        Ok(())
    }

    pub fn receive_rst(&self) {
        self.state.set(SenderState::Reset);
    }

    pub fn remote_ack(&self, ack_seq_no: SeqNumber, now: Instant) -> Result<(), Fail> {
        if self.state.get() == SenderState::SentFin
            && ack_seq_no == self.base_seq_no.get() + Wrapping(1)
        {
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
            return Err(Fail::Ignored {
                details: "ACK is outside of send window",
            });
        }
        if bytes_acknowledged == Wrapping(0) {
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
                return Err(Fail::Ignored {
                    details: "ACK isn't on segment boundary",
                });
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

    pub fn pop_one_unsent_byte(&self) -> Option<RT::Buf> {
        let mut queue = self.unsent_queue.borrow_mut();

        let buf = queue.front_mut()?;
        let mut cloned_buf = buf.clone();
        let buf_len = buf.len();

        // Pop one byte off the buf still in the queue and all but one of the bytes on our clone.
        buf.adjust(1);
        cloned_buf.trim(buf_len - 1);

        Some(cloned_buf)
    }

    pub fn pop_unsent(&self, max_bytes: usize) -> Option<RT::Buf> {
        // TODO: Use a scatter/gather array to coalesce multiple buffers into a single segment.
        let mut unsent_queue = self.unsent_queue.borrow_mut();
        let mut buf = unsent_queue.pop_front()?;
        let buf_len = buf.len();

        if buf_len > max_bytes {
            let mut cloned_buf = buf.clone();

            buf.adjust(max_bytes);
            cloned_buf.trim(buf_len - max_bytes);

            unsent_queue.push_front(buf);
            buf = cloned_buf;

        }
        Some(buf)
    }

    pub fn update_remote_window(&self, window_size_hdr: u16) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored {
                details: "Dropping remote window update for closed sender",
            });
        }

        // TODO: Is this the right check?
        let window_size = (window_size_hdr as u32)
            .checked_shl(self.window_scale as u32)
            .ok_or_else(|| Fail::Ignored {
                details: "Window size overflow",
            })?;

        debug!(
            "Updating window size -> {} (hdr {}, scale {})",
            window_size, window_size_hdr, self.window_scale
        );
        self.window_size.set(window_size);

        Ok(())
    }

    pub fn remote_mss(&self) -> usize {
        self.mss
    }

    pub fn current_rto(&self) -> Duration {
        self.rto.borrow().estimate()
    }
}
