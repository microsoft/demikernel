use crate::{
    collections::watched::WatchedValue,
    fail::Fail,
    protocols::tcp::SeqNumber,
    runtime::Runtime,
};
use std::{
    cell::RefCell,
    collections::{
        BTreeMap,
        VecDeque,
    },
    convert::TryInto,
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

const RECV_QUEUE_SZ: usize = 2048;
const MAX_OUT_OF_ORDER: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiverState {
    Open,
    ReceivedFin,
    AckdFin,
}

#[derive(Debug)]
pub struct Receiver<RT: Runtime> {
    pub state: WatchedValue<ReceiverState>,

    //                     |-----------------recv_window-------------------|
    //                base_seq_no             ack_seq_no             recv_seq_no
    //                     v                       v                       v
    // ... ----------------|-----------------------|-----------------------| (unavailable)
    //         received           acknowledged           unacknowledged
    //
    // NB: We can have `ack_seq_no < base_seq_no` when the application fully drains the receive
    // buffer before we've sent a pure ACK or transmitted some data on which we could piggyback
    // an ACK. The sender, however, will still be computing the receive window relative to the
    // the old `ack_seq_no` until we send them an ACK (see the diagram in sender.rs).
    //
    pub base_seq_no: WatchedValue<SeqNumber>,
    pub recv_queue: RefCell<VecDeque<RT::Buf>>,
    pub ack_seq_no: WatchedValue<SeqNumber>,
    pub recv_seq_no: WatchedValue<SeqNumber>,

    pub ack_deadline: WatchedValue<Option<Instant>>,

    pub max_window_size: u32,
    pub window_scale: u32,

    waker: RefCell<Option<Waker>>,
    out_of_order: RefCell<BTreeMap<SeqNumber, RT::Buf>>,
}

impl<RT: Runtime> Receiver<RT> {
    pub fn new(seq_no: SeqNumber, max_window_size: u32, window_scale: u32) -> Self {
        Self {
            state: WatchedValue::new(ReceiverState::Open),
            base_seq_no: WatchedValue::new(seq_no),
            recv_queue: RefCell::new(VecDeque::with_capacity(RECV_QUEUE_SZ)),
            ack_seq_no: WatchedValue::new(seq_no),
            recv_seq_no: WatchedValue::new(seq_no),
            ack_deadline: WatchedValue::new(None),
            max_window_size,
            window_scale,
            waker: RefCell::new(None),
            out_of_order: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn hdr_window_size(&self) -> u16 {
        let Wrapping(bytes_outstanding) = self.recv_seq_no.get() - self.base_seq_no.get();
        let window_size = self.max_window_size - bytes_outstanding;
        let hdr_window_size = (window_size >> self.window_scale)
            .try_into()
            .expect("Window size overflow");
        debug!(
            "Sending window size update -> {} (hdr {}, scale {})",
            (hdr_window_size as u32) << self.window_scale,
            hdr_window_size,
            self.window_scale
        );
        hdr_window_size
    }

    pub fn current_ack(&self) -> Option<SeqNumber> {
        let ack_seq_no = self.ack_seq_no.get();
        let recv_seq_no = self.recv_seq_no.get();
        if ack_seq_no != recv_seq_no {
            Some(recv_seq_no)
        } else {
            None
        }
    }

    pub fn ack_sent(&self, seq_no: SeqNumber) {
        if self.state.get() == ReceiverState::AckdFin {
            assert_eq!(seq_no, self.recv_seq_no.get() + Wrapping(1));
        } else {
            assert_eq!(seq_no, self.recv_seq_no.get());
        }
        self.ack_deadline.set(None);
        self.ack_seq_no.set(seq_no);
    }

    pub fn peek(&self) -> Result<RT::Buf, Fail> {
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

    pub fn recv(&self) -> Result<Option<RT::Buf>, Fail> {
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

    pub fn poll_recv(&self, ctx: &mut Context) -> Poll<Result<RT::Buf, Fail>> {
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

    pub fn receive_data(&self, seq_no: SeqNumber, buf: RT::Buf, now: Instant) -> Result<(), Fail> {
        if self.state.get() != ReceiverState::Open {
            return Err(Fail::ResourceNotFound {
                details: "Receiver closed",
            });
        }

        let recv_seq_no = self.recv_seq_no.get();
        if seq_no > recv_seq_no {
            let mut out_of_order = self.out_of_order.borrow_mut();
            if !out_of_order.contains_key(&seq_no) {
                while out_of_order.len() > MAX_OUT_OF_ORDER {
                    let (&key, _) = out_of_order.iter().rev().next().unwrap();
                    out_of_order.remove(&key);
                }
                out_of_order.insert(seq_no, buf);
                return Err(Fail::Ignored {
                    details: "Out of order segment (reordered)",
                });
            }
        }
        if seq_no < recv_seq_no {
            return Err(Fail::Ignored {
                details: "Out of order segment (duplicate)",
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

        let new_recv_seq_no = self.recv_seq_no.get();
        let old_data = {
            let mut out_of_order = self.out_of_order.borrow_mut();
            out_of_order.remove(&new_recv_seq_no)
        };
        if let Some(old_data) = old_data {
            info!("Recovering out-of-order packet at {}", new_recv_seq_no);
            if let Err(e) = self.receive_data(new_recv_seq_no, old_data, now) {
                info!("Failed to recover out-of-order packet: {:?}", e);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Receiver;
    use crate::{
        fail::Fail,
        sync::BytesMut,
    };
    use must_let::must_let;
    use std::{
        num::Wrapping,
        time::Instant,
    };
    use crate::test_helpers::TestRuntime;

    #[test]
    fn test_out_of_order() {
        let now = Instant::now();
        let receiver = Receiver::<TestRuntime>::new(Wrapping(0), 65536, 0);
        let buf = BytesMut::zeroed(16).freeze();
        must_let!(let Err(Fail::Ignored { .. }) = receiver.receive_data(Wrapping(16), buf.clone(), now));
        must_let!(let Ok(..) = receiver.receive_data(Wrapping(0), buf.clone(), now));
        assert_eq!(receiver.recv_seq_no.get(), Wrapping(32))
    }
}
