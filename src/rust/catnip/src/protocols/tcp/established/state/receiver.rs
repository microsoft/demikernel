use crate::{
    collections::watched::WatchedValue,
    fail::Fail,
    protocols::tcp::SeqNumber,
    sync::Bytes,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    num::Wrapping,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::{
        Duration,
        Instant,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiverState {
    Open,
    ReceivedFin,
    AckdFin,
}

#[derive(Debug)]
pub struct Receiver {
    pub state: WatchedValue<ReceiverState>,

    //                     |-----------------recv_window-------------------|
    //                base_seq_no             ack_seq_no             recv_seq_no
    //                     v                       v                       v
    // ... ----------------|-----------------------|-----------------------| (unavailable)
    //         received           acknowledged           unacknowledged
    //
    pub base_seq_no: WatchedValue<SeqNumber>,
    pub recv_queue: RefCell<VecDeque<Bytes>>,
    pub ack_seq_no: WatchedValue<SeqNumber>,
    pub recv_seq_no: WatchedValue<SeqNumber>,

    pub ack_deadline: WatchedValue<Option<Instant>>,

    pub max_window_size: u32,

    waker: RefCell<Option<Waker>>,
}

impl Receiver {
    pub fn new(seq_no: SeqNumber, max_window_size: u32) -> Self {
        Self {
            state: WatchedValue::new(ReceiverState::Open),
            base_seq_no: WatchedValue::new(seq_no),
            recv_queue: RefCell::new(VecDeque::new()),
            ack_seq_no: WatchedValue::new(seq_no),
            recv_seq_no: WatchedValue::new(seq_no),
            ack_deadline: WatchedValue::new(None),
            max_window_size,
            waker: RefCell::new(None),
        }
    }

    pub fn window_size(&self) -> u32 {
        let Wrapping(bytes_outstanding) = self.recv_seq_no.get() - self.base_seq_no.get();
        self.max_window_size - bytes_outstanding
    }

    pub fn current_ack(&self) -> Option<SeqNumber> {
        let ack_seq_no = self.ack_seq_no.get();
        let recv_seq_no = self.recv_seq_no.get();
        if ack_seq_no < recv_seq_no {
            Some(recv_seq_no)
        } else {
            None
        }
    }

    pub fn ack_sent(&self, seq_no: SeqNumber) {
        assert_eq!(seq_no, self.recv_seq_no.get());
        self.ack_deadline.set(None);
        self.ack_seq_no.set(seq_no);
    }

    pub fn peek(&self) -> Result<Bytes, Fail> {
        if self.base_seq_no.get() == self.recv_seq_no.get() {
            if self.state.get() != ReceiverState::Open {
                return Err(Fail::ResourceNotFound {
                    details: "Receiver closed",
                });
            }
            return Err(Fail::ResourceExhausted {
                details: "No available data",
            });
        }

        let segment = self
            .recv_queue
            .borrow_mut()
            .front()
            .expect("recv_seq > base_seq without data in queue?")
            .clone();

        Ok(segment)
    }

    pub fn recv(&self) -> Result<Option<Bytes>, Fail> {
        if self.base_seq_no.get() == self.recv_seq_no.get() {
            if self.state.get() != ReceiverState::Open {
                return Err(Fail::ResourceNotFound {
                    details: "Receiver closed",
                });
            }
            return Ok(None);
        }

        let segment = self
            .recv_queue
            .borrow_mut()
            .pop_front()
            .expect("recv_seq > base_seq without data in queue?");
        self.base_seq_no
            .modify(|b| b + Wrapping(segment.len() as u32));

        Ok(Some(segment))
    }

    pub fn poll_recv(&self, ctx: &mut Context) -> Poll<Result<Bytes, Fail>> {
        if self.base_seq_no.get() == self.recv_seq_no.get() {
            if self.state.get() != ReceiverState::Open {
                return Poll::Ready(Err(Fail::ResourceNotFound {
                    details: "Receiver closed",
                }));
            }
            *self.waker.borrow_mut() = Some(ctx.waker().clone());
            return Poll::Pending;
        }

        let segment = self
            .recv_queue
            .borrow_mut()
            .pop_front()
            .expect("recv_seq > base_seq without data in queue?");
        self.base_seq_no
            .modify(|b| b + Wrapping(segment.len() as u32));

        Poll::Ready(Ok(segment))
    }

    pub fn receive_fin(&self) {
        // Even if we've already ACKd the FIN, we need to resend the ACK if we receive another FIN.
        self.state.set(ReceiverState::ReceivedFin);
    }

    pub fn receive_data(&self, seq_no: SeqNumber, buf: Bytes, now: Instant) -> Result<(), Fail> {
        if self.state.get() != ReceiverState::Open {
            return Err(Fail::ResourceNotFound {
                details: "Receiver closed",
            });
        }

        if self.recv_seq_no.get() != seq_no {
            return Err(Fail::Ignored {
                details: "Out of order segment",
            });
        }

        let unread_bytes = self
            .recv_queue
            .borrow()
            .iter()
            .map(|b| b.len())
            .sum::<usize>();
        if unread_bytes + buf.len() > self.max_window_size as usize {
            return Err(Fail::Ignored {
                details: "Full receive window",
            });
        }

        self.recv_seq_no.modify(|r| r + Wrapping(buf.len() as u32));
        self.recv_queue.borrow_mut().push_back(buf);
        self.waker.borrow_mut().take().map(|w| w.wake());

        // TODO: How do we handle when the other side is in PERSIST state here?
        if self.ack_deadline.get().is_none() {
            // TODO: Configure this value (and also maybe just have an RT pointer here.)
            self.ack_deadline
                .set(Some(now + Duration::from_millis(500)));
        }

        Ok(())
    }
}
