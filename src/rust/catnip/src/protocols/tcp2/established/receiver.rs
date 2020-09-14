use crate::protocols::{arp, ip, ipv4};
use crate::protocols::tcp2::SeqNumber;
use std::cmp;
use std::convert::TryInto;
use std::future::Future;
use std::pin::Pin;
use crate::collections::watched::WatchedValue;
use std::collections::VecDeque;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::HashMap;
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use crate::runtime::Runtime;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use super::rto::RtoCalculator;
use futures::FutureExt;
use futures::future::{self, Either};

pub struct ReceiverControlBlock {
    open: WatchedValue<bool>,

    //                     |-----------------recv_window-------------------|
    //                base_seq_no             ack_seq_no             recv_seq_no
    //                     v                       v                       v
    // ... ----------------|-----------------------|-----------------------| (unavailable)
    //         received           acknowledged           unacknowledged
    //
    base_seq_no: WatchedValue<SeqNumber>,
    recv_queue: RefCell<VecDeque<Vec<u8>>>,
    ack_seq_no: WatchedValue<SeqNumber>,
    recv_seq_no: WatchedValue<SeqNumber>,

    ack_deadline: WatchedValue<Option<Instant>>,

    max_window_size: u32,

    local: ipv4::Endpoint,
    remote: ipv4::Endpoint,

    rt: Runtime,
    arp: arp::Peer,
}

impl ReceiverControlBlock {
    pub async fn acknowledger(receiver: Rc<ReceiverControlBlock>) -> ! {
        loop {
            let (open, open_changed) = receiver.open.watch();
            futures::pin_mut!(open_changed);

            if !open {
                open_changed.await;
                continue;
            }

            // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
            // - TCP should implement a delayed ACK
            // - The delay must be less than 500ms
            // - For a stream of full-sized segments, there should be an ack for every other segment.

            // TODO: Implement SACKs
            let (ack_deadline, ack_deadline_changed) = receiver.ack_deadline.watch();
            futures::pin_mut!(ack_deadline_changed);

            let ack_future = match ack_deadline {
                Some(t) => Either::Left(receiver.rt.wait_until(t).fuse()),
                None => Either::Right(future::pending()),
            };
            futures::pin_mut!(ack_future);

            futures::select_biased! {
                _ = open_changed => continue,
                _ = ack_deadline_changed => continue,
                _ = ack_future => {
                    let recv_seq_no = receiver.recv_seq_no.get();
                    assert!(receiver.ack_seq_no.get() < recv_seq_no);

                    let segment = TcpSegment::default()
                        .src_ipv4_addr(receiver.local.address())
                        .src_port(receiver.local.port())
                        .dest_ipv4_addr(receiver.remote.address())
                        .dest_port(receiver.remote.port())
                        .window_size(receiver.window_size() as usize)
                        .ack(recv_seq_no);

                    // Query ARP before modifying any of our data structures.
                    let remote_link_addr = match receiver.arp.query(receiver.remote.address()).await {
                        Ok(r) => r,
                        // TODO: What exactly should we do here?
                        Err(..) => continue,
                    };

                    let mut segment_buf = segment.encode();
                    let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                    encoder.ipv4().header().src_addr(receiver.rt.options().my_ipv4_addr);

                    let mut frame_header = encoder.ipv4().frame().header();
                    frame_header.src_addr(receiver.rt.options().my_link_addr);
                    frame_header.dest_addr(remote_link_addr);
                    let _ = encoder.seal().expect("TODO");

                    // TODO: We should have backpressure here for emitting events.
                    receiver.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));
                    receiver.ack_sent(receiver.recv_seq_no.get());
                },
            }
        }
    }
}

impl ReceiverControlBlock {
    pub fn window_size(&self) -> u32 {
        let Wrapping(bytes_outstanding) = self.recv_seq_no.get() - self.base_seq_no.get();
        self.max_window_size - bytes_outstanding
    }

    pub fn current_ack(&self) -> Option<SeqNumber> {
        let ack_seq_no = self.ack_seq_no.get();
        let recv_seq_no = self.recv_seq_no.get();
        if ack_seq_no < recv_seq_no { Some(recv_seq_no) } else { None }
    }

    pub fn ack_sent(&self, seq_no: SeqNumber) {
        assert_eq!(seq_no, self.recv_seq_no.get());
        self.ack_deadline.set(None);
        self.ack_seq_no.set(seq_no);
    }

    pub fn recv(&self) -> Result<Option<Vec<u8>>, Fail> {
        if !self.open.get() {
            return Err(Fail::ResourceNotFound { details: "Receiver closed" });
        }

        if self.base_seq_no.get() == self.recv_seq_no.get() {
            return Ok(None);
        }

        let segment = self.recv_queue.borrow_mut().pop_front()
            .expect("recv_seq > base_seq without data in queue?");
        self.base_seq_no.modify(|b| b + Wrapping(segment.len() as u32));

        Ok(Some(segment))
    }

    pub fn receive_segment(&self, seq_no: SeqNumber, buf: Vec<u8>) -> Result<(), Fail> {
        if !self.open.get() {
            return Err(Fail::ResourceNotFound { details: "Receiver closed" });
        }

        if self.recv_seq_no.get() != seq_no {
            return Err(Fail::Ignored { details: "Out of order segment" });
        }

        let unread_bytes = self.recv_queue.borrow().iter().map(|b| b.len()).sum::<usize>();
        if unread_bytes + buf.len() > self.max_window_size as usize {
            return Err(Fail::Ignored { details: "Full receive window" });
        }

        self.recv_seq_no.modify(|r| r + Wrapping(buf.len() as u32));
        self.recv_queue.borrow_mut().push_back(buf);

        // TODO: How do we handle when the other side is in PERSIST state here?
        if self.ack_deadline.get().is_none() {
            // TODO: Configure this value.
            self.ack_deadline.set(Some(self.rt.now() + Duration::from_millis(500)));
        }

        Ok(())
    }
}
