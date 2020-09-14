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
use super::receiver::ReceiverControlBlock;

pub struct UnackedSegment {
    pub bytes: Vec<u8>,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

pub struct SenderControlBlock {
    open: WatchedValue<bool>,

    // TODO: Just use Figure 5 from RFC 793 here.
    //
    //                    |------------window_size------------|
    //
    //               base_seq_no               sent_seq_no           unsent_seq_no
    //                    v                         v                      v
    // ... ---------------|-------------------------|----------------------| (unavailable)
    //       acknowleged         unacknowledged     ^        unsent
    //
    base_seq_no: WatchedValue<SeqNumber>,
    unacked_queue: RefCell<VecDeque<UnackedSegment>>,
    sent_seq_no: WatchedValue<SeqNumber>,
    unsent_queue: RefCell<VecDeque<Vec<u8>>>,
    unsent_seq_no: WatchedValue<SeqNumber>,

    window_size: WatchedValue<u32>,
    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    window_scale: u8,

    mss: usize,

    retransmit_deadline: WatchedValue<Option<Instant>>,
    rto: RefCell<RtoCalculator>,

    pub local: ipv4::Endpoint,
    pub remote: ipv4::Endpoint,

    pub rt: Runtime,
    pub arp: arp::Peer,
}

impl SenderControlBlock {
    pub async fn retransmitter(sender: Rc<SenderControlBlock>) -> ! {
        loop {
            let (open, open_changed) = sender.open.watch();
            futures::pin_mut!(open_changed);

            if !open {
                open_changed.await;
                continue;
            }

            let (rtx_deadline, rtx_deadline_changed) = sender.retransmit_deadline.watch();
            futures::pin_mut!(rtx_deadline_changed);

            let rtx_future = match rtx_deadline {
                Some(t) => Either::Left(sender.rt.wait_until(t).fuse()),
                None => Either::Right(future::pending()),
            };
            futures::pin_mut!(rtx_future);
            futures::select_biased! {
                _ = open_changed => continue,
                _ = rtx_deadline_changed => continue,
                _ = rtx_future => {
                    let mut unacked_queue = sender.unacked_queue.borrow_mut();
                    let mut rto = sender.rto.borrow_mut();

                    // Our retransmission timer fired, so we need to resend a packet.
                    let segment = match unacked_queue.front_mut() {
                        Some(s) => s,
                        None => panic!("Retransmission timer set with empty acknowledge queue"),
                    };

                    // TODO: Congestion control
                    rto.record_failure();

                    // Unset the initial timestamp so we don't use this for RTT estimation.
                    segment.initial_tx.take();

                    // TODO: Repacketization
                    // XXX: Actually make a real packet
                    sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment.bytes.clone()))));

                    // Set new retransmit deadline
                    let deadline = sender.rt.now() + rto.estimate();
                    sender.retransmit_deadline.set(Some(deadline));
                },
            }
        }
    }

    pub async fn initial_sender(sender: Rc<SenderControlBlock>, receiver: Rc<ReceiverControlBlock>) -> ! {
        'top: loop {
            // TODO: Come up with a nice way to accumulate preconditions.
            let (open, open_changed) = sender.open.watch();
            futures::pin_mut!(open_changed);

            if !open {
                open_changed.await;
                continue;
            }

            // Next, check to see if there's any unsent data.
            let (unsent_seq, unsent_seq_changed) = sender.unsent_seq_no.watch();
            futures::pin_mut!(unsent_seq_changed);

            // We don't need to watch this value since we're the only mutator.
            let sent_seq_no = sender.sent_seq_no.get();

            if sent_seq_no == unsent_seq {
                futures::select_biased! {
                    _ = open_changed => continue 'top,
                    _ = unsent_seq_changed => continue 'top,
                }
            }

            let (win_sz, win_sz_changed) = sender.window_size.watch();
            futures::pin_mut!(win_sz_changed);

            // If we don't have any window space at all, send window probes starting at one RTO and
            // exponentially increasing *forever*
            if win_sz == 0 {
                // Query ARP before modifying any of our data structures.
                let remote_link_addr = match sender.arp.query(sender.remote.address()).await {
                    Ok(r) => r,
                    // TODO: What exactly should we do here?
                    Err(..) => continue,
                };
                let buf = {
                    let mut queue = sender.unsent_queue.borrow_mut();
                    let mut buf = queue.pop_front().expect("No unsent data?");
                    let remainder = buf.split_off(1);
                    queue.push_front(remainder);
                    buf
                };

                let mut segment = TcpSegment::default()
                    .src_ipv4_addr(sender.local.address())
                    .src_port(sender.local.port())
                    .dest_ipv4_addr(sender.remote.address())
                    .dest_port(sender.remote.port())
                    .window_size(receiver.window_size() as usize)
                    .seq_num(sent_seq_no)
                    .payload(buf.clone());
                let rx_ack = receiver.current_ack();
                if let Some(ack_seq_no) = rx_ack {
                    segment = segment.ack(ack_seq_no);
                }
                let mut segment_buf = segment.encode();
                let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                encoder.ipv4().header().src_addr(sender.rt.options().my_ipv4_addr);

                let mut frame_header = encoder.ipv4().frame().header();
                frame_header.src_addr(sender.rt.options().my_link_addr);
                frame_header.dest_addr(remote_link_addr);
                let _ = encoder.seal().expect("TODO");
                sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

                if sender.retransmit_deadline.get().is_none() {
                    let deadline = sender.rt.now() + sender.rto.borrow().estimate();
                    sender.retransmit_deadline.set(Some(deadline));
                }
                sender.sent_seq_no.modify(|s| s + Wrapping(buf.len() as u32));
                let unacked_segment = UnackedSegment {
                    bytes: buf.clone(),
                    initial_tx: Some(sender.rt.now()),
                };
                sender.unacked_queue.borrow_mut().push_back(unacked_segment);

                // TODO: Use the correct PERSIST state timer here.
                let mut timeout = Duration::from_secs(1);
                loop {
                    futures::select_biased! {
                        _ = open_changed => continue 'top,
                        _ = win_sz_changed => continue 'top,
                        _ = sender.rt.wait(timeout).fuse() => {
                            timeout *= 2;
                        }
                    }
                    // Forcibly retransmit.
                    let mut segment = TcpSegment::default()
                        .src_ipv4_addr(sender.local.address())
                        .src_port(sender.local.port())
                        .dest_ipv4_addr(sender.remote.address())
                        .dest_port(sender.remote.port())
                        .window_size(receiver.window_size() as usize)
                        .seq_num(sent_seq_no)
                        .payload(buf.clone());
                    let rx_ack = receiver.current_ack();
                    if let Some(ack_seq_no) = rx_ack {
                        segment = segment.ack(ack_seq_no);
                    }
                    let mut segment_buf = segment.encode();
                    let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
                    encoder.ipv4().header().src_addr(sender.rt.options().my_ipv4_addr);

                    let mut frame_header = encoder.ipv4().frame().header();
                    frame_header.src_addr(sender.rt.options().my_link_addr);
                    frame_header.dest_addr(remote_link_addr);
                    let _ = encoder.seal().expect("TODO");
                    sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

                    if sender.retransmit_deadline.get().is_none() {
                        let deadline = sender.rt.now() + sender.rto.borrow().estimate();
                        sender.retransmit_deadline.set(Some(deadline));
                    }
                }
            }

            let (base_seq, base_seq_changed) = sender.base_seq_no.watch();
            futures::pin_mut!(base_seq_changed);

            // Wait until we have space in the window to send some data. Note that we're the only ones
            // who actually send data, so since we've established above that there's data waiting, we
            // don't have to watch that value again.
            let Wrapping(sent_data) = sent_seq_no - base_seq;
            if win_sz <= sent_data {
                futures::select_biased! {
                    _ = open_changed => continue 'top,
                    _ = win_sz_changed => continue 'top,
                    _ = base_seq_changed => continue 'top,
                }
            }

            // TODO: Nagle's algorithm
            // TODO: Silly window syndrome

            // Query ARP before modifying any of our data structures.
            let remote_link_addr = match sender.arp.query(sender.remote.address()).await {
                Ok(r) => r,
                // TODO: What exactly should we do here?
                Err(..) => continue,
            };

            let mut bytes_remaining = cmp::min((win_sz - sent_data) as usize, sender.mss);
            let mut segment_data = vec![];
            {
                let mut unsent_queue = sender.unsent_queue.borrow_mut();
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
            }
            let segment_data_len = segment_data.len();
            assert!(!segment_data.is_empty());

            let mut segment = TcpSegment::default()
                .src_ipv4_addr(sender.local.address())
                .src_port(sender.local.port())
                .dest_ipv4_addr(sender.remote.address())
                .dest_port(sender.remote.port())
                .window_size(receiver.window_size() as usize)
                .seq_num(sent_seq_no)
                .payload(segment_data.clone());

            let rx_ack = receiver.current_ack();
            if let Some(ack_seq_no) = rx_ack {
                segment = segment.ack(ack_seq_no);
            }

            let mut segment_buf = segment.encode();
            let mut encoder = TcpSegmentEncoder::attach(&mut segment_buf);
            encoder.ipv4().header().src_addr(sender.rt.options().my_ipv4_addr);

            let mut frame_header = encoder.ipv4().frame().header();
            frame_header.src_addr(sender.rt.options().my_link_addr);
            frame_header.dest_addr(remote_link_addr);
            let _ = encoder.seal().expect("TODO");

            // TODO: We should have backpressure here for emitting events.
            sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment_buf))));

            if sender.retransmit_deadline.get().is_none() {
                let deadline = sender.rt.now() + sender.rto.borrow().estimate();
                sender.retransmit_deadline.set(Some(deadline));
            }

            sender.sent_seq_no.modify(|s| s + Wrapping(segment_data.len() as u32));
            let unacked_segment = UnackedSegment {
                bytes: segment_data,
                initial_tx: Some(sender.rt.now()),
            };
            sender.unacked_queue.borrow_mut().push_back(unacked_segment);

            if let Some(ack_seq_no) = rx_ack {
                receiver.ack_sent(ack_seq_no);
            }
        }
    }
}

impl SenderControlBlock {
    pub fn send(&self, buf: Vec<u8>) -> Result<(), Fail> {
        if !self.open.get() {
            return Err(Fail::Ignored { details: "Sender closed" });
        }
        let buf_len: u32 = buf.len().try_into()
            .map_err(|_| Fail::Ignored { details: "Buffer too large" })?;;
        self.unsent_queue.borrow_mut().push_back(buf);
        self.unsent_seq_no.modify(|s| s + Wrapping(buf_len));
        Ok(())
    }

    pub fn remote_ack(&self, ack_seq_no: SeqNumber) -> Result<(), Fail> {
        if !self.open.get() {
            return Err(Fail::Ignored { details: "Dropping remote ACK for closed sender" });
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
            let deadline = self.rt.now() + self.rto.borrow().estimate();
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
                self.rto.borrow_mut().add_sample(self.rt.now() - initial_tx);
            }
            if bytes_remaining == 0 {
                break;
            }
        }
        self.base_seq_no.modify(|b| b + bytes_acknowledged);

        Ok(())
    }

    pub fn update_remote_window(&self, window_size_hdr: u16) -> Result<(), Fail> {
        if !self.open.get() {
            return Err(Fail::Ignored { details: "Dropping remote window update for closed sender" });
        }

        // TODO: Is this the right check?
        let window_size = (window_size_hdr as u32).checked_shl(window_size_hdr as u32)
            .ok_or_else(|| Fail::Ignored { details: "Window size overflow" })?;
        self.window_size.set(window_size);

        Ok(())
    }
}
