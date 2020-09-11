#![allow(unused)]

// TODO:
// [ ] Half closed states?
// [ ] Timeouts
// [ ] Options from https://man7.org/linux/man-pages/man7/tcp.7.html
// [ ] Connection state machine

use crate::protocols::tcp::segment::TcpSegment;
use crate::collections::watched::WatchedValue;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Instant, Duration};
use crate::runtime::Runtime;
use futures::{FutureExt, StreamExt};
use futures::future;
use futures::future::Either;
use futures::channel::mpsc;
use float_duration::FloatDuration;
use crate::fail::Fail;
use std::cmp;
use crate::event::Event;
use std::cell::RefCell;

// RFC6298
struct RtoCalculator {
    srtt: f64,
    rttvar: f64,
    rto: f64,

    received_sample: bool,
}

impl RtoCalculator {
    fn new() -> Self {
        Self {
            srtt: 1.0,
            rttvar: 0.0,
            rto: 1.0,

            received_sample: false,
        }
    }

    pub fn add_sample(&mut self, rtt: Duration) {
        const ALPHA: f64 = 0.125;
        const BETA: f64 = 0.25;
        const GRANULARITY: f64 = 0.001f64;

        let rtt = FloatDuration::from(rtt).as_seconds();

        if !self.received_sample {
            self.srtt = rtt;
            self.rttvar = rtt / 2.;
            self.received_sample = true;
        } else {
            self.rttvar = (1.0 - BETA) * self.rttvar + BETA * (self.srtt - rtt).abs();
            self.srtt = (1.0 - ALPHA) * self.srtt + ALPHA * rtt;
        }

        let rttvar_x4 = match (4.0 * self.rttvar).partial_cmp(&GRANULARITY) {
            Some(cmp::Ordering::Less) => GRANULARITY,
            None => panic!("NaN rttvar: {:?}", self.rttvar),
            _ => self.rttvar,
        };
        self.update_rto(self.srtt + rttvar_x4);
    }

    fn update_rto(&mut self, new_rto: f64) {
        const UBOUND_SEC: f64 = 60.0f64;
        const LBOUND_SEC: f64 = 0.001f64;
        self.rto = match (new_rto.partial_cmp(&LBOUND_SEC), new_rto.partial_cmp(&UBOUND_SEC)) {
            (Some(cmp::Ordering::Less), _) => LBOUND_SEC,
            (_, Some(cmp::Ordering::Greater)) => UBOUND_SEC,
            (None, _) | (_, None) => panic!("NaN RTO: {:?}", new_rto),
            _ => new_rto
        };
    }

    pub fn record_failure(&mut self) {
        self.update_rto(self.rto * 2.0);
    }

    pub fn estimate(&self) -> Duration {
        FloatDuration::seconds(self.rto).to_std().unwrap()
    }
}

// TODO: Shouldn't this just be a wrapping u32?
type SeqNumber = i32;

struct UnackedSegment {
    bytes: Vec<u8>,
    // Set to `None` on retransmission to implement Karn's algorithm.
    initial_tx: Option<Instant>,
}

struct Sender {

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

    retransmit_deadline: WatchedValue<Option<Instant>>,

    rto: RefCell<RtoCalculator>,
    rt: Runtime,
}

fn acknowledge(sender: Rc<Sender>, ack_seq_no: SeqNumber) -> Result<(), Fail> {
    let mut rto = sender.rto.borrow_mut();

    let base_seq_no = sender.base_seq_no.get();
    let bytes_acknowledged = ack_seq_no.wrapping_sub(base_seq_no);

    if bytes_acknowledged < 0 {
        warn!("Discarding old ACK for {}", ack_seq_no);
        return Ok(());
    }
    // TODO: Handle fast retransmit here.
    if bytes_acknowledged == 0 {
        warn!("Dropping duplicate ACK at {}", ack_seq_no);
        return Ok(());
    }
    let bytes_acknowledged = bytes_acknowledged as usize;

    let sent_seq_no = sender.sent_seq_no.get();
    if ack_seq_no > sent_seq_no {
        return Err(Fail::Ignored { details: "ACK is outside of send window" });
    }
    if ack_seq_no == sent_seq_no {
        // If we've acknowledged all sent data, turn off the retransmit timer.
        sender.retransmit_deadline.set(None);
    } else {
        // Otherwise, set it to the current RTO.
        let deadline = sender.rt.now() + rto.estimate();
        sender.retransmit_deadline.set(Some(deadline));
    }

    // TODO: Do acks need to be on segment boundaries? How does this interact with repacketization?
    let mut bytes_dequeued = 0;
    let mut unacked_queue = sender.unacked_queue.borrow_mut();
    while let Some(segment) = unacked_queue.pop_front() {
        bytes_dequeued += segment.bytes.len();
        if bytes_dequeued > bytes_acknowledged {
            return Err(Fail::Ignored { details: "ACK isn't on segment boundary" });
        }
        assert!(bytes_dequeued <= bytes_acknowledged, "ACK not on segment boundary");

        // Add sample for RTO if not a retransmission
        // TODO: TCP timestamp support.
        if let Some(initial_tx) = segment.initial_tx {
            rto.add_sample(sender.rt.now() - initial_tx);
        }
        if bytes_dequeued == bytes_acknowledged {
            break;
        }
    }
    sender.base_seq_no.modify(|b| b + bytes_dequeued as i32);

    Ok(())
}

fn update_window(sender: Rc<Sender>, window_size_hdr: u16) -> Result<(), Fail> {
    // TODO: Check for overflow
    let window_size = (window_size_hdr as u32) << sender.window_scale;
    sender.window_size.set(window_size);

    Ok(())
}

// TODO: Backpressure here if buffer is too full.
fn write(sender: Rc<Sender>, buf: Vec<u8>) {
    sender.unsent_seq_no.modify(|s| s.wrapping_add(buf.len() as i32));
    sender.unsent_queue.borrow_mut().push_back(buf);
}

async fn retransmitter(sender: Rc<Sender>) -> ! {
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
                // TODO: Actually make a real packet
                sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment.bytes.clone()))));

                // Set new retransmit deadline
                let deadline = sender.rt.now() + rto.estimate();
                sender.retransmit_deadline.set(Some(deadline));
            },
        }
    }
}

async fn sender(sender: Rc<Sender>) -> ! {
    'top: loop {
        let (win_sz, win_sz_changed) = sender.window_size.watch();
        futures::pin_mut!(win_sz_changed);

        // If we don't have any window space at all, send window probes starting at one RTO and
        // exponentially increasing *forever*
        if win_sz == 0 {
            let mut timeout = Duration::from_secs(1);
            loop {
                // TODO: Send window probe here.
                futures::select_biased! {
                    _ = win_sz_changed => continue 'top,
                    _ = sender.rt.wait(timeout).fuse() => {
                        timeout *= 2;
                    }
                }
            }
        }

        // Next, check to see if there's any unsent data.
        let (unsent_seq, unsent_seq_changed) = sender.unsent_seq_no.watch();
        futures::pin_mut!(unsent_seq_changed);

        // We don't need to watch this value since we're the only mutator.
        let sent_seq_no = sender.sent_seq_no.get();

        if unsent_seq == sent_seq_no {
            futures::select_biased! {
                _ = win_sz_changed => continue 'top,
                _ = unsent_seq_changed => continue 'top,
            }
        }

        let (base_seq, base_seq_changed) = sender.base_seq_no.watch();
        futures::pin_mut!(base_seq_changed);

        // TODO: Better numeric types.
        // Wait until we have space in the window to send some data. Note that we're the only ones
        // who actually send data, so since we've established above that there's data waiting, we
        // don't have to watch that value again.
        let window_end = base_seq as usize + win_sz as usize;
        if window_end <= sent_seq_no as usize {
            futures::select_biased! {
                _ = win_sz_changed => continue 'top,
                _ = base_seq_changed => continue 'top,
            }
        }

        // TODO: Nagle's algorithm
        // TODO: Silly window syndrome
        let mut unsent_queue = sender.unsent_queue.borrow_mut();
        let mut available_window = window_end - sent_seq_no as usize;
        let mut segment = vec![];
        while available_window > 0 {
            let mut buf = match unsent_queue.pop_front() {
                Some(b) => b,
                None => break,
            };
            if buf.len() > available_window {
                let tail = buf.split_off(available_window);
                unsent_queue.push_front(tail);
            }
            available_window -= buf.len();
            segment.extend(buf);
        }
        assert!(!segment.is_empty());
        if sender.retransmit_deadline.get().is_none() {
            let deadline = sender.rt.now() + sender.rto.borrow().estimate();
            sender.retransmit_deadline.set(Some(deadline));

        }
        sender.sent_seq_no.modify(|s| s + segment.len() as i32);
        let unacked_segment = UnackedSegment {
            bytes: segment.clone(),
            initial_tx: Some(sender.rt.now()),
        };
        sender.unacked_queue.borrow_mut().push_back(unacked_segment);

        // TODO: We should have backpressure here for emitting events.
        // TODO: Actually make a real packet
        sender.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(segment))));
    }
}

struct Receiver {

    //                     |-----------------recv_window-------------------|
    //                base_seq_no             ack_seq_no             recv_seq_no
    //                     v                       v                       v
    // ... ----------------|-----------------------|-----------------------| (unavailable)
    //         received           acknowledged           unacknowledged
    //

    base_seq_no: WatchedValue<SeqNumber>,
    ack_seq_no: RefCell<SeqNumber>,
    recv_seq_no: WatchedValue<SeqNumber>,

    recv_queue: RefCell<VecDeque<Vec<u8>>>,
    max_window_size: usize,

    ack_deadline: WatchedValue<Option<Instant>>,

    rt: Runtime,
}


fn receive(receiver: Rc<Receiver>, seq_no: SeqNumber, segment: Vec<u8>) -> Result<(), Fail> {
    if receiver.recv_seq_no.get() != seq_no {
        warn!("Dropping out of order segment at {}", seq_no);
        return Ok(());
    }

    let unread_bytes = receiver.recv_queue.borrow().iter().map(|b| b.len()).sum::<usize>();
    if unread_bytes + segment.len() > receiver.max_window_size {
        warn!("Dropping packet due to full window: {} + {} > {}", unread_bytes, segment.len(), receiver.max_window_size);
        return Ok(());
    }

    receiver.recv_seq_no.modify(|r| r + segment.len() as i32);
    receiver.recv_queue.borrow_mut().push_back(segment);

    if receiver.ack_deadline.get().is_none() {
        receiver.ack_deadline.set(Some(receiver.rt.now() + Duration::from_millis(500)));
    }

    Ok(())
}


async fn read(receiver: Rc<Receiver>) -> Vec<u8> {
    loop {
        let (recv_seq, recv_seq_changed) = receiver.recv_seq_no.watch();

        if receiver.base_seq_no.get() == recv_seq {
            recv_seq_changed.await;
            continue;
        }

        let segment = receiver.recv_queue.borrow_mut().pop_front()
            .expect("recv_seq > base_seq without data in queue?");
        receiver.base_seq_no.modify(|b| b + segment.len() as i32);

        return segment;
    }
}

async fn acknowledger(receiver: Rc<Receiver>) -> ! {
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
            // TODO: Send acknowledgements in sender.
            _ = ack_deadline_changed => continue,
            _ = ack_future => {
                assert!(*receiver.ack_seq_no.borrow() < receiver.recv_seq_no.get());

                // TODO: Send the ACK.
                receiver.rt.emit_event(Event::Transmit(Rc::new(RefCell::new(vec![]))));
                receiver.ack_deadline.set(None);
                *receiver.ack_seq_no.borrow_mut() = receiver.recv_seq_no.get();
            },
        }
    }
}

async fn active_open() {
    let handshake_timeout = Duration::from_secs(1);
    let handshake_retries = 2;
    let mss = 1024;

    let syn_segment = TcpSegment::default()
    // .connection(..)
        .mss(mss)
        .syn();

    // Send a SYN packet, exponentially retrying
    let syn_tx = async {
        for i in 0..handshake_retries {
            syn_segment.cast();
            rt.timeout(handshake_timeout).await?;
        }
        Err(..)
    };
    // Await a SYN+ACK packet, discarding irrelevant ones.
    let syn_ack_rx = async {
        while let Some(segment) = incoming_segments.receive().await {
            if segment.rst {
                return Err(..);
            }
            if segment.ack && segment.syn == syn_segment.syn && segment.ack_num == syn_segment.seq_num + Wrapping(1) {
                return Some(segment);
            }
        }
        Err(..)
    };

    // Set remote isn
    // Negotiate MSS (?)
    // Set remote receive window size
    // incr seq
    // Send ACK packet
    let segment = TcpSegment::default();

    // Create ESTABLISHED state

    // Process incoming segments (?) and feed them into the stack.
    let mut sender = Some(sender);
    let mut receiver = Some(receiver);

    loop {
        futures::select_biased! {
            segment = incoming_segments.receive() => {

            },
            _ = retransmitter => (),
            _ = sender => (),
            _ = acknowledger => (),
        }
    }

    while let Some(segment) = incoming_segments.receive().await {
        if segment.rst {
        }
    }
}

enum ConnectionState {
    Closed,

    Connecting { connect_future: () },
    Established { sender: Sender, receiver: Receiver },

    CloseWait { sender: Sender },
    FinWait { receiver: Receiver },

    TimeWait,
}


async fn connection() {
    let sender_st: Rc<Sender> = unimplemented!();
    let receiver_st: Rc<Receiver> = unimplemented!();

    // Connection establishment
    // - CLOSED
    //   - Active Open(a) => Send(SYN) => SYN_SENT
    //   - Passive Open(p) => LISTEN
    // - SYN_SENT
    //   - Recv(SYN) => Send(SYN+ACK) => SYN_RCVD
    //   - Recv(SYN+ACK) => Send(ACK) => ESTABLISHED
    //   - Close | Timeout => CLOSED
    // - LISTEN
    //   - Recv(SYN) => Send(SYN+ACK) => SYN_RCVD
    // - SYN_RCVD
    //   - Recv(RST) => LISTEN
    //   - Recv(ACK) => ESTABLISHED
    //   - Close => Send(FIN) => FIN_WAIT_1 (maybe disallow this by only giving the FD once ESTABLISHED?)

    // Connection close
    // - ESTABLISHED
    //   - Recv(FIN) => Send(ACK) => CLOSE_WAIT (passive close)
    //   - Close => Send(FIN) => FIN_WAIT_1 (active close)
    // - CLOSE_WAIT (EOF on rx stream)
    //   - Close => Send(FIN) => LAST_ACK
    // - LAST_ACK
    //   - Recv(ACK) => CLOSED
    // - FIN_WAIT_1
    //   - Recv(ACK) => FIN_WAIT_2
    //   - Recv(FIN+ACK) => TIME_WAIT
    //   - Recv(FIN) => Send(ACK) => CLOSING
    // - FIN_WAIT_2
    //   - Recv(FIN) => Send(ACK) => TIME_WAIT
    // - CLOSING
    //   - Recv(ACK) => TIME_WAIT
    // - TIME_WAIT
    //   - 2MSL Timeout => CLOSED

    // Negotiate MSS
    //
    // Connect future completion
    // Accept future completion

    // NB: Options are only available here.
    let snd = async {
        futures::select_biased! {
            _ = retransmitter(sender_st.clone()).fuse() => (),
            _ = sender(sender_st.clone()).fuse() => (),
        }
    };
    let rcv = async {
        acknowledger(receiver_st).await
    };
    futures::select_biased! {
        _ = snd.fuse() => (),
        _ = rcv.fuse() => (),
        // TODO: cancel snd/rcv processes on state change.
    }

    // Transition to half-closed/closed
}
