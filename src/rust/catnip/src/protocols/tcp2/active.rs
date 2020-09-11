use crate::protocols::{arp, ip, ipv4};
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

type ConnectFuture = impl Future<Output = ()>;

fn connect() -> ConnectFuture {
    async {
    }
}

enum State {
    Connecting,
    Established,
    CloseWait,
    FinWait,
}

// impl Sock2 {
//     fn new() -> Self {
//         Self {
//             sender: None,
//             receiver: None,
//             connect: Some(connect()),
//         }
//     }
// }

pub type SeqNumber = Wrapping<u32>;

pub struct ActiveSocket {
    sender: Option<Sender>,
    receiver: Option<Receiver>,
}

struct UnackedSegment {
    bytes: Vec<u8>,
    // Set to `None` on retransmission to implement Karn's algorithm.
    initial_tx: Option<Instant>,
}

impl ActiveSocket {
    pub fn new() -> Self {
        unimplemented!();
    }

    pub fn receive_segment(&self, segment: TcpSegment) -> Result<(), Fail> {
        // self.inbox.try_send(segment)
        //     .map_err(|_| Fail::ResourceBusy { details: "Active socket backlog overflow" })?;
        unimplemented!();
    }

    pub fn send(&self, buf: Vec<u8>) -> Result<(), Fail> {
        let sender = self.sender.as_ref()
            .ok_or_else(|| Fail::Ignored { details: "Dropping window update for closed sender" })?;
        let scb = &sender.control_block;

        let buf_len: u32 = buf.len().try_into()
            .map_err(|_| Fail::Ignored { details: "Buffer too large" })?;;

        scb.unsent_queue.borrow_mut().push_back(buf);
        scb.unsent_seq_no.modify(|s| s + Wrapping(buf_len));

        Ok(())
    }

    pub fn recv(&self) -> Result<Option<Vec<u8>>, Fail> {
        let receiver = self.receiver.as_ref()
            .ok_or_else(|| Fail::ResourceNotFound { details: "Receiver closed" })?;
        let rcb = &receiver.control_block;

        if rcb.base_seq_no.get() == rcb.recv_seq_no.get() {
            return Ok(None);
        }

        let segment = rcb.recv_queue.borrow_mut().pop_front()
            .expect("recv_seq > base_seq without data in queue?");
        rcb.base_seq_no.modify(|b| b + Wrapping(segment.len() as u32));

        Ok(Some(segment))
    }

    fn remote_acknowledge(&self, ack_seq_no: SeqNumber) -> Result<(), Fail> {
        let sender = self.sender.as_ref()
            .ok_or_else(|| Fail::Ignored { details: "Dropping ACK for closed sender" })?;
        let scb = &sender.control_block;

        let base_seq_no = scb.base_seq_no.get();
        let sent_seq_no = scb.sent_seq_no.get();

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
            scb.retransmit_deadline.set(None);
        } else {
            // Otherwise, set it to the current RTO.
            let deadline = scb.rt.now() + scb.rto.borrow().estimate();
            scb.retransmit_deadline.set(Some(deadline));
        }

        // TODO: Do acks need to be on segment boundaries? How does this interact with repacketization?
        let mut bytes_remaining = bytes_acknowledged.0 as usize;
        while let Some(segment) = scb.unacked_queue.borrow_mut().pop_front() {
            if segment.bytes.len() > bytes_remaining {
                // TODO: We need to close the connection in this case.
                return Err(Fail::Ignored { details: "ACK isn't on segment boundary" });
            }
            bytes_remaining -= segment.bytes.len();

            // Add sample for RTO if not a retransmission
            // TODO: TCP timestamp support.
            if let Some(initial_tx) = segment.initial_tx {
                scb.rto.borrow_mut().add_sample(scb.rt.now() - initial_tx);
            }
            if bytes_remaining == 0 {
                break;
            }
        }
        scb.base_seq_no.modify(|b| b + bytes_acknowledged);

        Ok(())
    }

    fn update_remote_window(&self, window_size_hdr: u16) -> Result<(), Fail> {
        let sender = self.sender.as_ref()
            .ok_or_else(|| Fail::Ignored { details: "Dropping window update for closed sender" })?;
        let scb = &sender.control_block;

        // TODO: Is this the right check?
        let window_size = (window_size_hdr as u32).checked_shl(window_size_hdr as u32)
            .ok_or_else(|| Fail::Ignored { details: "Window size overflow" })?;
        scb.window_size.set(window_size);

        Ok(())
    }

    fn local_receive(&self, seq_no: SeqNumber, buf: Vec<u8>) -> Result<(), Fail> {
        let receiver = self.receiver.as_ref()
            .ok_or_else(|| Fail::Ignored { details: "Dropping receive for closed receiver" })?;
        let rcb = &receiver.control_block;

        if rcb.recv_seq_no.get() != seq_no {
            return Err(Fail::Ignored { details: "Out of order segment" });
        }

        let unread_bytes = rcb.recv_queue.borrow().iter().map(|b| b.len()).sum::<usize>();
        if unread_bytes + buf.len() > rcb.max_window_size as usize {
            return Err(Fail::Ignored { details: "Full receive window" });
        }

        rcb.recv_seq_no.modify(|r| r + Wrapping(buf.len() as u32));
        rcb.recv_queue.borrow_mut().push_back(buf);

        // TODO: How do we handle when the other side is in PERSIST state here?
        if rcb.ack_deadline.get().is_none() {
            // TODO: Configure this value.
            rcb.ack_deadline.set(Some(rcb.rt.now() + Duration::from_millis(500)));
        }

        Ok(())
    }
}

struct Sender {
    control_block: Rc<SenderControlBlock>,
    retransmitter: Pin<Box<dyn Future<Output = !>>>,
    sender: Pin<Box<dyn Future<Output = !>>>,
}

impl Sender {
    async fn retransmitter(sender: Rc<SenderControlBlock>) -> ! {
        loop {
            let (rtx_deadline, rtx_deadline_changed) = sender.retransmit_deadline.watch();
            futures::pin_mut!(rtx_deadline_changed);

            let rtx_future = match rtx_deadline {
                Some(t) => Either::Left(sender.rt.wait_until(t).fuse()),
                None => Either::Right(future::pending()),
            };
            futures::pin_mut!(rtx_future);
            futures::select_biased! {
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

    async fn sender(sender: Rc<SenderControlBlock>, receiver: Rc<ReceiverControlBlock>) -> ! {
        'top: loop {
            // Next, check to see if there's any unsent data.
            let (unsent_seq, unsent_seq_changed) = sender.unsent_seq_no.watch();
            futures::pin_mut!(unsent_seq_changed);

            // We don't need to watch this value since we're the only mutator.
            let sent_seq_no = sender.sent_seq_no.get();

            if sent_seq_no == unsent_seq {
                unsent_seq_changed.await;
                continue 'top;
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
                let rx_ack = receiver.ack();
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
                    let rx_ack = receiver.ack();
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

            let rx_ack = receiver.ack();
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
                receiver.ack_deadline.set(None);
                receiver.ack_seq_no.set(ack_seq_no);
            }
        }
    }
}

struct SenderControlBlock {
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

    local: ipv4::Endpoint,
    remote: ipv4::Endpoint,

    rt: Runtime,
    arp: arp::Peer,
}

struct Receiver {
    control_block: Rc<ReceiverControlBlock>,
    acknowledger: Pin<Box<dyn Future<Output = !>>>,
}

impl Receiver {
    async fn acknowledger(receiver: Rc<ReceiverControlBlock>) -> ! {
        loop {
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

                    receiver.ack_deadline.set(None);
                    receiver.ack_seq_no.set(receiver.recv_seq_no.get());
                },
            }
        }
    }
}



struct ReceiverControlBlock {
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
    fn window_size(&self) -> u32 {
        let Wrapping(bytes_outstanding) = self.recv_seq_no.get() - self.base_seq_no.get();
        self.max_window_size - bytes_outstanding
    }

    fn ack(&self) -> Option<SeqNumber> {
        let ack_seq_no = self.ack_seq_no.get();
        let recv_seq_no = self.recv_seq_no.get();
        if ack_seq_no < recv_seq_no { Some(recv_seq_no) } else { None }
    }
}
