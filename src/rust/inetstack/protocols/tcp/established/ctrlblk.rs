// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    congestion_control::{
        self,
        CongestionControlConstructor,
    },
    rto::RtoCalculator,
    sender::{
        Sender,
        UnackedSegment,
    },
};
use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            segment::{
                TcpHeader,
                TcpSegment,
            },
            SeqNumber,
        },
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            config::TcpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        timer::TimerRc,
        watched::{
            WatchFuture,
            WatchedValue,
        },
    },
    scheduler::scheduler::Scheduler,
};
use ::std::{
    cell::{
        Cell,
        RefCell,
        RefMut,
    },
    collections::VecDeque,
    convert::TryInto,
    net::SocketAddrV4,
    rc::Rc,
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

// ToDo: Review this value (and its purpose).  It (2048 segments) of 8 KB jumbo packets would limit the unread data to
// just 16 MB.  If we don't want to lie, that is also about the max window size we should ever advertise.  Whereas TCP
// with the window scale option allows for window sizes of up to 1 GB.  This value appears to exist more because of the
// mechanism used to manage the receive queue (a VecDeque) than anything else.
const RECV_QUEUE_SZ: usize = 2048;

// ToDo: Review this value (and its purpose).  It (16 segments) seems awfully small (would make fast retransmit less
// useful), and this mechanism isn't the best way to protect ourselves against deliberate out-of-order segment attacks.
// Ideally, we'd limit out-of-order data to that which (along with the unread data) will fit in the receive window.
const MAX_OUT_OF_ORDER: usize = 16;

// TCP Connection State.
// Note: This ControlBlock structure is only used after we've reached the ESTABLISHED state, so states LISTEN,
// SYN_RCVD, and SYN_SENT aren't included here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
    CloseWait,
    LastAck,
    Closed,
}

// ToDo: Consider incorporating this directly into ControlBlock.
struct Receiver {
    //
    // Receive Sequence Space:
    //
    //                     |<---------------receive_buffer_size---------------->|
    //                     |                                                    |
    //                     |                         |<-----receive window----->|
    //                reader_next              receive_next       receive_next + receive window
    //                     v                         v                          v
    // ... ----------------|-------------------------|--------------------------|------------------------------
    //      read by user   |  received but not read  |    willing to receive    | future sequence number space
    //
    // Note: In RFC 793 terminology, receive_next is RCV.NXT, and "receive window" is RCV.WND.
    //

    // Sequence number of next byte of data in the unread queue.
    pub reader_next: Cell<SeqNumber>,

    // Sequence number of the next byte of data (or FIN) that we expect to receive.  In RFC 793 terms, this is RCV.NXT.
    pub receive_next: Cell<SeqNumber>,

    // Receive queue.  Contains in-order received (and acknowledged) data ready for the application to read.
    recv_queue: RefCell<VecDeque<DemiBuffer>>,
}

impl Receiver {
    pub fn new(reader_next: SeqNumber, receive_next: SeqNumber) -> Self {
        Self {
            reader_next: Cell::new(reader_next),
            receive_next: Cell::new(receive_next),
            recv_queue: RefCell::new(VecDeque::with_capacity(RECV_QUEUE_SZ)),
        }
    }

    pub fn pop(&self, size: Option<usize>) -> Result<Option<DemiBuffer>, Fail> {
        let mut recv_queue: RefMut<VecDeque<DemiBuffer>> = self.recv_queue.borrow_mut();

        // Check if the receive queue is empty.
        if recv_queue.is_empty() {
            return Ok(None);
        }

        let buf: DemiBuffer = if let Some(size) = size {
            let buf: &mut DemiBuffer = recv_queue.front_mut().expect("receive queue cannot be empty");
            // Split the buffer if it's too big.
            if buf.len() > size {
                buf.split_front(size)?
            } else {
                recv_queue.pop_front().expect("receive queue cannot be empty")
            }
        } else {
            recv_queue.pop_front().expect("receive queue cannot be empty")
        };

        self.reader_next
            .set(self.reader_next.get() + SeqNumber::from(buf.len() as u32));

        Ok(Some(buf))
    }

    pub fn push(&self, buf: DemiBuffer) {
        let buf_len: u32 = buf.len() as u32;
        self.recv_queue.borrow_mut().push_back(buf);
        self.receive_next
            .set(self.receive_next.get() + SeqNumber::from(buf_len as u32));
    }
}

/// Transmission control block for representing our TCP connection.
// ToDo: Make all public fields in this structure private.
pub struct ControlBlock {
    local: SocketAddrV4,
    remote: SocketAddrV4,

    rt: Rc<dyn NetworkRuntime>,
    pub scheduler: Scheduler,
    pub clock: TimerRc,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,

    // ToDo: We shouldn't be keeping anything datalink-layer specific at this level.  The IP layer should be holding
    // this along with other remote IP information (such as routing, path MTU, etc).
    arp: Rc<ArpPeer>,

    // Send-side state information.  ToDo: Consider incorporating this directly into ControlBlock.
    sender: Sender,

    // TCP Connection State.
    state: Cell<State>,

    ack_delay_timeout: Duration,

    ack_deadline: WatchedValue<Option<Instant>>,

    // This is our receive buffer size, which is also the maximum size of our receive window.
    // Note: The maximum possible advertised window is 1 GiB with window scaling and 64 KiB without.
    receive_buffer_size: u32,

    // ToDo: Review how this is used.  We could have separate window scale factors, so there should be one for the
    // receiver and one for the sender.
    // This is the receive-side window scale factor.
    // This is the number of bits to shift to convert to/from the scaled value, and has a maximum value of 14.
    // ToDo: Keep this as a u8?
    window_scale: u32,

    waker: RefCell<Option<Waker>>,

    // Queue of out-of-order segments.  This is where we hold onto data that we've received (because it was within our
    // receive window) but can't yet present to the user because we're missing some other data that comes between this
    // and what we've already presented to the user.
    //
    out_of_order: RefCell<VecDeque<(SeqNumber, DemiBuffer)>>,

    // The sequence number of the FIN, if we received it out-of-order.
    // Note: This could just be a boolean to remember if we got a FIN; the sequence number is for checking correctness.
    pub out_of_order_fin: Cell<Option<SeqNumber>>,

    // Receive-side state information.  ToDo: Consider incorporating this directly into ControlBlock.
    receiver: Receiver,

    // Whether the user has called close.
    pub user_is_done_sending: Cell<bool>,

    // Congestion control trait implementation we're currently using.
    // ToDo: Consider switching this to a static implementation to avoid V-table call overhead.
    cc: Box<dyn congestion_control::CongestionControl>,

    // Current retransmission timer expiration time.
    // ToDo: Consider storing this directly in the RtoCalculator.
    retransmit_deadline: WatchedValue<Option<Instant>>,

    // Retransmission Timeout (RTO) calculator.
    rto_calculator: RefCell<RtoCalculator>,
}

//==============================================================================

impl ControlBlock {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        receiver_seq_no: SeqNumber,
        ack_delay_timeout: Duration,
        receiver_window_size: u32,
        receiver_window_scale: u32,
        sender_seq_no: SeqNumber,
        sender_window_size: u32,
        sender_window_scale: u8,
        sender_mss: usize,
        cc_constructor: CongestionControlConstructor,
        congestion_control_options: Option<congestion_control::Options>,
    ) -> Self {
        let sender = Sender::new(sender_seq_no, sender_window_size, sender_window_scale, sender_mss);
        Self {
            local,
            remote,
            rt,
            scheduler,
            clock,
            local_link_addr,
            tcp_config,
            arp: Rc::new(arp),
            sender: sender,
            state: Cell::new(State::Established),
            ack_delay_timeout,
            ack_deadline: WatchedValue::new(None),
            receive_buffer_size: receiver_window_size,
            window_scale: receiver_window_scale,
            waker: RefCell::new(None),
            out_of_order: RefCell::new(VecDeque::new()),
            out_of_order_fin: Cell::new(Option::None),
            receiver: Receiver::new(receiver_seq_no, receiver_seq_no),
            user_is_done_sending: Cell::new(false),
            cc: cc_constructor(sender_mss, sender_seq_no, congestion_control_options),
            retransmit_deadline: WatchedValue::new(None),
            rto_calculator: RefCell::new(RtoCalculator::new()),
        }
    }

    pub fn get_local(&self) -> SocketAddrV4 {
        self.local
    }

    pub fn get_remote(&self) -> SocketAddrV4 {
        self.remote
    }

    // ToDo: Remove this.  ARP doesn't belong at this layer.
    pub fn arp(&self) -> Rc<ArpPeer> {
        self.arp.clone()
    }

    pub fn send(&self, buf: DemiBuffer) -> Result<(), Fail> {
        self.sender.send(buf, self)
    }

    pub fn retransmit(&self) {
        self.sender.retransmit(self)
    }

    pub fn congestion_control_watch_retransmit_now_flag(&self) -> (bool, WatchFuture<bool>) {
        self.cc.watch_retransmit_now_flag()
    }

    pub fn congestion_control_on_fast_retransmit(&self) {
        self.cc.on_fast_retransmit()
    }

    pub fn congestion_control_on_rto(&self, send_unacknowledged: SeqNumber) {
        self.cc.on_rto(send_unacknowledged)
    }

    pub fn congestion_control_on_send(&self, rto: Duration, num_sent_bytes: u32) {
        self.cc.on_send(rto, num_sent_bytes)
    }

    pub fn congestion_control_on_cwnd_check_before_send(&self) {
        self.cc.on_cwnd_check_before_send()
    }

    pub fn congestion_control_get_cwnd(&self) -> u32 {
        self.cc.get_cwnd()
    }

    pub fn congestion_control_watch_cwnd(&self) -> (u32, WatchFuture<u32>) {
        self.cc.watch_cwnd()
    }

    pub fn congestion_control_get_limited_transmit_cwnd_increase(&self) -> u32 {
        self.cc.get_limited_transmit_cwnd_increase()
    }

    pub fn congestion_control_watch_limited_transmit_cwnd_increase(&self) -> (u32, WatchFuture<u32>) {
        self.cc.watch_limited_transmit_cwnd_increase()
    }

    pub fn get_mss(&self) -> usize {
        self.sender.get_mss()
    }

    pub fn get_send_window(&self) -> (u32, WatchFuture<u32>) {
        self.sender.get_send_window()
    }

    pub fn get_send_unacked(&self) -> (SeqNumber, WatchFuture<SeqNumber>) {
        self.sender.get_send_unacked()
    }

    pub fn get_unsent_seq_no(&self) -> (SeqNumber, WatchFuture<SeqNumber>) {
        self.sender.get_unsent_seq_no()
    }

    pub fn get_send_next(&self) -> (SeqNumber, WatchFuture<SeqNumber>) {
        self.sender.get_send_next()
    }

    pub fn modify_send_next(&self, f: impl FnOnce(SeqNumber) -> SeqNumber) {
        self.sender.modify_send_next(f)
    }

    pub fn get_retransmit_deadline(&self) -> Option<Instant> {
        self.retransmit_deadline.get()
    }

    pub fn set_retransmit_deadline(&self, when: Option<Instant>) {
        self.retransmit_deadline.set(when);
    }

    pub fn watch_retransmit_deadline(&self) -> (Option<Instant>, WatchFuture<Option<Instant>>) {
        self.retransmit_deadline.watch()
    }

    pub fn push_unacked_segment(&self, segment: UnackedSegment) {
        self.sender.push_unacked_segment(segment)
    }

    pub fn rto_add_sample(&self, rtt: Duration) {
        self.rto_calculator.borrow_mut().add_sample(rtt)
    }

    pub fn rto(&self) -> Duration {
        self.rto_calculator.borrow().rto()
    }

    pub fn rto_back_off(&self) {
        self.rto_calculator.borrow_mut().back_off()
    }

    pub fn unsent_top_size(&self) -> Option<usize> {
        self.sender.top_size_unsent()
    }

    pub fn pop_unsent_segment(&self, max_bytes: usize) -> Option<DemiBuffer> {
        self.sender.pop_unsent(max_bytes)
    }

    pub fn pop_one_unsent_byte(&self) -> Option<DemiBuffer> {
        self.sender.pop_one_unsent_byte()
    }

    // This is the main TCP receive routine.
    //
    pub fn receive(&self, mut header: &mut TcpHeader, mut data: DemiBuffer) {
        debug!(
            "{:?} Connection Receiving {} bytes + {:?}",
            self.state.get(),
            data.len(),
            header
        );

        let mut should_schedule_ack: bool = false;

        // ToDo: We're probably getting "now" here in order to get a timestamp as close as possible to when we received
        // the packet.  However, this is wasteful if we don't take a path below that actually uses it.  Review this.
        let now: Instant = self.clock.now();

        // Check to see if the segment is acceptable sequence-wise (i.e. contains some data that fits within the receive
        // window, or is a non-data segment with a sequence number that falls within the window).  Unacceptable segments
        // should be ACK'd (unless they are RSTs), and then dropped.
        //
        // [From RFC 793]
        // There are four cases for the acceptability test for an incoming segment:
        //
        // Segment Receive  Test
        // Length  Window
        // ------- -------  -------------------------------------------
        //
        //   0       0     SEG.SEQ = RCV.NXT
        //
        //   0      >0     RCV.NXT =< SEG.SEQ < RCV.NXT+RCV.WND
        //
        //  >0       0     not acceptable
        //
        //  >0      >0     RCV.NXT =< SEG.SEQ < RCV.NXT+RCV.WND
        //              or RCV.NXT =< SEG.SEQ+SEG.LEN-1 < RCV.NXT+RCV.WND

        // Review: We don't need all of these intermediate variables in the fast path.  It might be more efficient to
        // rework this to calculate some of them only when needed, even if we need to (re)do it in multiple places.

        let mut seg_start: SeqNumber = header.seq_num;

        let mut seg_end: SeqNumber = seg_start;
        let mut seg_len: u32 = data.len() as u32;
        if header.syn {
            seg_len += 1;
        }
        if header.fin {
            seg_len += 1;
        }
        if seg_len > 0 {
            seg_end = seg_start + SeqNumber::from(seg_len - 1);
        }

        let receive_next: SeqNumber = self.receiver.receive_next.get();

        let after_receive_window: SeqNumber = receive_next + SeqNumber::from(self.get_receive_window_size());

        // Check if this segment fits in our receive window.
        // In the optimal case it starts at RCV.NXT, so we check for that first.
        //
        if seg_start != receive_next {
            // The start of this segment is not what we expected.  See if it comes before or after.
            //
            if seg_start < receive_next {
                // This segment contains duplicate data (i.e. data we've already received).
                // See if it is a complete duplicate, or if some of the data is new.
                //
                if seg_end < receive_next {
                    // This is an entirely duplicate (i.e. old) segment.  ACK (if not RST) and drop.
                    //
                    if !header.rst {
                        self.send_ack();
                    }
                    return;
                } else {
                    // Some of this segment's data is new.  Cut the duplicate data off of the front.
                    // If there is a SYN at the start of this segment, remove it too.
                    //
                    let mut duplicate: u32 = u32::from(receive_next - seg_start);
                    seg_start = seg_start + SeqNumber::from(duplicate);
                    seg_len -= duplicate;
                    if header.syn {
                        header.syn = false;
                        duplicate -= 1;
                    }
                    data.adjust(duplicate as usize)
                        .expect("'data' should contain at least 'duplicate' bytes");
                }
            } else {
                // This segment contains entirely new data, but is later in the sequence than what we're expecting.
                // See if any part of the data fits within our receive window.
                //
                if seg_start >= after_receive_window {
                    // This segment is completely outside of our window.  ACK (if not RST) and drop.
                    //
                    if !header.rst {
                        self.send_ack();
                    }
                    return;
                }

                // At least the beginning of this segment is in the window.  We'll check the end below.
            }
        }

        // The start of the segment is in the window.
        // Check that the end of the segment is in the window, and trim it down if it is not.
        //
        if seg_len > 0 && seg_end >= after_receive_window {
            let mut excess: u32 = u32::from(seg_end - after_receive_window);
            excess += 1;
            // ToDo: If we end up (after receive handling rewrite is complete) not needing seg_end and seg_len after
            // this, remove these two lines adjusting them as they're being computed needlessly.
            seg_end = seg_end - SeqNumber::from(excess);
            seg_len -= excess;
            if header.fin {
                header.fin = false;
                excess -= 1;
            }
            data.trim(excess as usize)
                .expect("'data' should contain at least 'excess' bytes");
        }

        // From here on, the entire new segment (including any SYN or FIN flag remaining) is in the window.
        // Note that one interpretation of RFC 793 would have us store away (or just drop) any out-of-order packets at
        // this point, and only proceed onwards if seg_start == receive_next.  But we process any RSTs, SYNs, or ACKs
        // we receive (as long as they're in the window) as we receive them, even if they're out-of-order.  It's only
        // when we get to processing the data (and FIN) that we store aside any out-of-order segments for later.
        debug_assert!(receive_next <= seg_start && seg_end < after_receive_window);

        // Check the RST bit.
        if header.rst {
            // ToDo: RFC 5961 "Blind Reset Attack Using the RST Bit" prevention would have us ACK and drop if the new
            // segment doesn't start precisely on RCV.NXT.

            // Our peer has given up.  Shut the connection down hard.
            info!("Received RST");
            match self.state.get() {
                // Data transfer states.
                State::Established | State::FinWait1 | State::FinWait2 | State::CloseWait => {
                    // ToDo: Return all outstanding user Receive and Send requests with "reset" responses.
                    // ToDo: Flush all segment queues.

                    // Enter Closed state.
                    self.state.set(State::Closed);

                    // ToDo: Delete the ControlBlock.
                    return;
                },

                // Closing states.
                State::Closing | State::LastAck | State::TimeWait => {
                    // Enter Closed state.
                    self.state.set(State::Closed);

                    // ToDo: Delete the ControlBlock.
                    return;
                },

                // Should never happen.
                state => panic!("Bad TCP state {:?}", state),
            }

            // Note: We should never get here.
        }

        // Note: RFC 793 says to check security/compartment and precedence next, but those are largely deprecated.

        // Check the SYN bit.
        if header.syn {
            // ToDo: RFC 5961 "Blind Reset Attack Using the SYN Bit" prevention would have us always ACK and drop here.

            // Receiving a SYN here is an error.
            warn!("Received in-window SYN on established connection.");
            // ToDo: Send Reset.
            // ToDo: Return all outstanding Receive and Send requests with "reset" responses.
            // ToDo: Flush all segment queues.

            // Enter Closed state.
            self.state.set(State::Closed);

            // ToDo: Delete the ControlBlock.
            return;
        }

        // Check the ACK bit.
        if !header.ack {
            // All segments on established connections should be ACKs.  Drop this segment.
            warn!("Received non-ACK segment on established connection.");
            return;
        }

        // ToDo: RFC 5961 "Blind Data Injection Attack" prevention would have us perform additional ACK validation
        // checks here.

        // Process the ACK.
        // Note: We process valid ACKs while in any synchronized state, even though there shouldn't be anything to do
        // in some states (e.g. TIME-WAIT) as it is more wasteful to always check that we're not in TIME-WAIT.
        //
        // Start by checking that the ACK acknowledges something new.
        // ToDo: Look into removing Watched types.
        //
        let (send_unacknowledged, _): (SeqNumber, _) = self.sender.get_send_unacked();
        let (send_next, _): (SeqNumber, _) = self.sender.get_send_next();

        // ToDo: Restructure this call into congestion control to either integrate it directly or make it more fine-
        // grained.  It currently duplicates the new/duplicate ack check itself internally, which is inefficient.
        // We should either make separate calls for each case or integrate those cases directly.
        self.cc.on_ack_received(
            self.rto_calculator.borrow().rto(),
            send_unacknowledged,
            send_next,
            header.ack_num,
        );

        if send_unacknowledged < header.ack_num {
            if header.ack_num <= send_next {
                // This segment acknowledges new data (possibly and/or FIN).
                let bytes_acknowledged: u32 = (header.ack_num - send_unacknowledged).into();

                // Remove the now acknowledged data from the unacknowledged queue.
                self.sender.remove_acknowledged_data(self, bytes_acknowledged, now);

                // Update SND.UNA to SEG.ACK.
                self.sender.send_unacked.set(header.ack_num);

                // Update our send window (SND.WND).
                self.sender.update_send_window(header);

                if header.ack_num == send_next {
                    // This segment acknowledges everything we've sent so far (i.e. nothing is currently outstanding).

                    // Since we no longer have anything outstanding, we can turn off the retransmit timer.
                    self.retransmit_deadline.set(None);

                    // Some states require additional processing.
                    match self.state.get() {
                        State::Established => (), // Common case.  Nothing more to do.
                        State::FinWait1 => {
                            // Our FIN is now ACK'd, so enter FIN-WAIT-2.
                            self.state.set(State::FinWait2);
                        },
                        State::Closing => {
                            // Our FIN is now ACK'd, so enter TIME-WAIT.
                            self.state.set(State::TimeWait);
                        },
                        State::LastAck => {
                            // Our FIN is now ACK'd, so this connection can be safely closed.  In LAST-ACK state we
                            // were just waiting for all of our sent data (including FIN) to be ACK'd, so now that it
                            // is, we can delete our state (we maintained it in case we needed to retransmit something,
                            // but we had already sent everything we're ever going to send (incl. FIN) at least once).
                            self.state.set(State::Closed);
                        },
                        // TODO: Handle TimeWait to Closed transition.
                        _ => (),
                    }
                } else {
                    // Update the retransmit timer.  Some of our outstanding data is now acknowledged, but not all.
                    // ToDo: This looks wrong.  We should reset the retransmit timer to match the deadline for the
                    // oldest still-outstanding data.  The below is overly generous (minor efficiency issue).
                    let deadline: Instant = now + self.rto_calculator.borrow().rto();
                    self.retransmit_deadline.set(Some(deadline));
                }
            } else {
                // This segment acknowledges data we have yet to send!?  Send an ACK and drop the segment.
                // ToDo: See RFC 5961, this could be a Blind Data Injection Attack.
                warn!("Received segment acknowledging data we have yet to send!");
                self.send_ack();
                return;
            }
        } else {
            // Duplicate ACK (doesn't acknowledge anything new).  We can mostly ignore this, except for fast-retransmit.
            // ToDo: Implement fast-retransmit.  In which case, we'd increment our dup-ack counter here.
        }

        // ToDo: Check the URG bit.  If we decide to support this, how should we do it?
        if header.urg {
            warn!("Got packet with URG bit set!");
        }

        // We can only process in-order data (or FIN).  Check for out-of-order segment.
        if seg_start != receive_next {
            debug!("Received out-of-order segment");
            // This segment is out-of-order.  If it carries data, and/or a FIN, we should store it for later processing
            // after the "hole" in the sequence number space has been filled.
            if seg_len > 0 {
                match self.state.get() {
                    State::Established | State::FinWait1 | State::FinWait2 => {
                        // We can only legitimately receive data in ESTABLISHED, FIN-WAIT-1, and FIN-WAIT-2.
                        if header.fin {
                            seg_len -= 1;
                            self.store_out_of_order_fin(seg_end);
                            seg_end = seg_end - SeqNumber::from(1);
                        }
                        debug_assert_eq!(seg_len, data.len() as u32);
                        if seg_len > 0 {
                            self.store_out_of_order_segment(seg_start, seg_end, data);
                        }
                        // Sending an ACK here is only a "MAY" according to the RFCs, but helpful for fast retransmit.
                        self.send_ack();
                    },
                    state => warn!("Ignoring data received after FIN (in state {:?}).", state),
                }
            }

            // We're done with this out-of-order segment.
            return;
        }

        // Process the segment text (if any).
        if !data.is_empty() {
            match self.state.get() {
                State::Established | State::FinWait1 | State::FinWait2 => {
                    // We can only legitimately receive data in ESTABLISHED, FIN-WAIT-1, and FIN-WAIT-2.
                    header.fin |= self.receive_data(seg_start, data);
                    should_schedule_ack = true;
                },
                state => warn!("Ignoring data received after FIN (in state {:?}).", state),
            }
        }

        // Check the FIN bit.
        if header.fin {
            trace!("Received FIN");

            // Advance RCV.NXT over the FIN.
            self.receiver
                .receive_next
                .set(self.receiver.receive_next.get() + SeqNumber::from(1));

            match self.state.get() {
                State::Established => self.state.set(State::CloseWait),
                State::FinWait1 => {
                    // RFC 793 has a benign logic flaw.  It says "If our FIN has been ACKed (perhaps in this segment),
                    // then enter TIME-WAIT, start the time-wait timer, turn off the other timers;".  But if our FIN
                    // has been ACK'd, we'd be in FIN-WAIT-2 here as a result of processing that ACK (see ACK handling
                    // above) and will enter TIME-WAIT in the FIN-WAIT-2 case below.  So we can skip that clause and go
                    // straight to "otherwise enter the CLOSING state".
                    self.state.set(State::Closing);
                },
                State::FinWait2 => {
                    // Enter TIME-WAIT.
                    self.state.set(State::TimeWait);
                    // ToDo: Start the time-wait timer and turn off the other timers.
                },
                State::CloseWait | State::Closing | State::LastAck => (), // Remain in current state.
                State::TimeWait => {
                    // ToDo: Remain in TIME-WAIT.  Restart the 2 MSL time-wait timeout.
                },
                state => panic!("Bad TCP state {:?}", state), // Should never happen.
            }

            // Push empty buffer.
            // TODO: set err bit and wake
            self.receiver.push(DemiBuffer::new(0));
            if let Some(w) = self.waker.borrow_mut().take() {
                w.wake()
            }

            // Since we consumed the FIN we ACK immediately rather than opportunistically.
            // ToDo: Consider doing this opportunistically.  Note our current tests expect the immediate behavior.
            self.send_ack();

            return;
        }

        // Check if we need to ACK soon.
        if should_schedule_ack {
            // We should ACK this segment, preferably via piggybacking on a response.
            // ToDo: Consider replacing the delayed ACK timer with a simple flag.
            if self.ack_deadline.get().is_none() {
                // Start the delayed ACK timer to ensure an ACK gets sent soon even if no piggyback opportunity occurs.
                self.ack_deadline.set(Some(now + self.ack_delay_timeout));
            } else {
                // We already owe our peer an ACK (the timer was already running), so cancel the timer and ACK now.
                self.ack_deadline.set(None);
                self.send_ack();
            }
        }
    }

    /// Handle the user's close request.
    ///
    /// In TCP parlance, a user's close request means "I have no more data to send".  The user may continue to receive
    /// data on this connection until their peer also closes.
    ///
    /// Note this routine will only be called for connections with a ControlBlock (i.e. in state ESTABLISHED or later).
    ///
    pub fn close(&self) -> Result<(), Fail> {
        // Check to see if close has already been called, as we should only do this once.
        if self.user_is_done_sending.get() {
            // Review: Should we return an error here instead?  RFC 793 recommends a "connection closing" error.
            return Ok(());
        }

        // In the normal case, we'll be in either ESTABLISHED or CLOSE_WAIT here (depending upon whether we've received
        // a FIN from our peer yet).  Queue up a FIN to be sent, and attempt to send it immediately (if possible).  We
        // only change state to FIN-WAIT-1 or LAST_ACK after we've actually been able to send the FIN.
        debug_assert!((self.state.get() == State::Established) || (self.state.get() == State::CloseWait));

        // Send a FIN.
        let fin_buf: DemiBuffer = DemiBuffer::new(0);
        self.send(fin_buf).expect("send failed");

        // TODO: Set state to FIN-WAIT1 if currently establisehd or set to LASTACK if CloseWait.

        // Remember that the user has called close.
        self.user_is_done_sending.set(true);

        Ok(())
    }

    /// Handle moving the connection to the closed state.
    ///
    /// This function runs the TCP state machine once it has either sent or received a FIN. This function is only for
    /// closing estabilished connections.
    ///
    pub fn poll_close(&self) -> Poll<Result<(), Fail>> {
        // TODO: Retry FIN if not successful.
        // TODO: Check if we have reached the CLOSED state, otherwise continue polling.
        // For now, just immediately return with ok.
        Poll::Ready(Ok(()))
    }

    /// Fetch a TCP header filling out various values based on our current state.
    /// ToDo: Fix the "filling out various values based on our current state" part to actually do that correctly.
    pub fn tcp_header(&self) -> TcpHeader {
        let mut header: TcpHeader = TcpHeader::new(self.local.port(), self.remote.port());
        header.window_size = self.hdr_window_size();

        // Note that once we reach a synchronized state we always include a valid acknowledgement number.
        header.ack = true;
        header.ack_num = self.receiver.receive_next.get();

        // Return this header.
        header
    }

    /// Send an ACK to our peer, reflecting our current state.
    pub fn send_ack(&self) {
        let mut header: TcpHeader = self.tcp_header();

        // ToDo: Think about moving this to tcp_header() as well.
        let (seq_num, _): (SeqNumber, _) = self.get_send_next();
        header.seq_num = seq_num;

        // ToDo: Remove this if clause once emit() is fixed to not require the remote hardware addr (this should be
        // left to the ARP layer and not exposed to TCP).
        if let Some(remote_link_addr) = self.arp().try_query(self.remote.ip().clone()) {
            self.emit(header, None, remote_link_addr);
        }
    }

    /// Transmit this message to our connected peer.
    ///
    pub fn emit(&self, header: TcpHeader, body: Option<DemiBuffer>, remote_link_addr: MacAddress) {
        // Only perform this debug print in debug builds.  debug_assertions is compiler set in non-optimized builds.
        #[cfg(debug_assertions)]
        if body.is_some() {
            debug!("Sending {} bytes + {:?}", body.as_ref().unwrap().len(), header);
        } else {
            debug!("Sending 0 bytes + {:?}", header);
        }

        // This routine should only ever be called to send TCP segments that contain a valid ACK value.
        debug_assert!(header.ack);

        let sent_fin: bool = header.fin;

        // Prepare description of TCP segment to send.
        // ToDo: Change this to call lower levels to fill in their header information, handle routing, ARPing, etc.
        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ipv4_hdr: Ipv4Header::new(self.local.ip().clone(), self.remote.ip().clone(), IpProtocol::TCP),
            tcp_hdr: header,
            data: body,
            tx_checksum_offload: self.tcp_config.get_tx_checksum_offload(),
        };

        // Call the runtime to send the segment.
        self.rt.transmit(Box::new(segment));

        // Post-send operations follow.
        // Review: We perform these after the send, in order to keep send latency as low as possible.

        // Since we sent an ACK, cancel any outstanding delayed ACK request.
        self.set_ack_deadline(None);

        // If we sent a FIN, update our protocol state.
        if sent_fin {
            match self.state.get() {
                // Active close.
                State::Established => self.state.set(State::FinWait1),
                // Passive close.
                State::CloseWait => self.state.set(State::LastAck),
                // We can legitimately retransmit the FIN in these states.  And we stay there until the FIN is ACK'd.
                State::FinWait1 | State::LastAck => {},
                // We shouldn't be sending a FIN from any other state.
                state => warn!("Sent FIN while in nonsensical TCP state {:?}", state),
            }
        }
    }

    pub fn remote_mss(&self) -> usize {
        self.sender.remote_mss()
    }

    pub fn get_ack_deadline(&self) -> (Option<Instant>, WatchFuture<Option<Instant>>) {
        self.ack_deadline.watch()
    }

    pub fn set_ack_deadline(&self, when: Option<Instant>) {
        self.ack_deadline.set(when);
    }

    pub fn get_receive_window_size(&self) -> u32 {
        let bytes_unread: u32 = (self.receiver.receive_next.get() - self.receiver.reader_next.get()).into();
        self.receive_buffer_size - bytes_unread
    }

    pub fn hdr_window_size(&self) -> u16 {
        let window_size: u32 = self.get_receive_window_size();
        let hdr_window_size: u16 = (window_size >> self.window_scale)
            .try_into()
            .expect("Window size overflow");
        debug!(
            "Window size -> {} (hdr {}, scale {})",
            (hdr_window_size as u32) << self.window_scale,
            hdr_window_size,
            self.window_scale
        );
        hdr_window_size
    }

    pub fn poll_recv(&self, ctx: &mut Context, size: Option<usize>) -> Poll<Result<DemiBuffer, Fail>> {
        // ToDo: Need to add a way to indicate that the other side closed (i.e. that we've received a FIN).
        // Should we do this via a zero-sized buffer?  Same as with the unsent and unacked queues on the send side?
        //
        // This code was checking for an empty receive queue by comparing sequence numbers, as in:
        //  if self.receiver.reader_next.get() == self.receiver.receive_next.get() {
        // But that will think data is available to be read once we've received a FIN, because FINs consume sequence
        // number space.  Now we call is_empty() on the receive queue instead.
        if self.receiver.recv_queue.borrow().is_empty() {
            *self.waker.borrow_mut() = Some(ctx.waker().clone());
            return Poll::Pending;
        }

        match self.receiver.pop(size) {
            Ok(Some(segment)) => Poll::Ready(Ok(segment)),
            Ok(None) => {
                warn!("poll_recv(): polling empty receive queue (ignoring spurious wake up)");
                Poll::Pending
            },
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    // This routine remembers that we have received an out-of-order FIN.
    //
    pub fn store_out_of_order_fin(&self, fin: SeqNumber) {
        self.out_of_order_fin.set(Some(fin));
    }

    // This routine takes an incoming TCP segment and adds it to the out-of-order receive queue.
    // If the new segment had a FIN it has been removed prior to this routine being called.
    // Note: Since this is not the "fast path", this is written for clarity over efficiency.
    //
    pub fn store_out_of_order_segment(&self, mut new_start: SeqNumber, mut new_end: SeqNumber, mut buf: DemiBuffer) {
        let mut out_of_order = self.out_of_order.borrow_mut();
        let mut action_index: usize = out_of_order.len();
        let mut another_pass_neeeded: bool = true;

        while another_pass_neeeded {
            another_pass_neeeded = false;

            // Find the new segment's place in the out-of-order store.
            // The out-of-order store is sorted by starting sequence number, and contains no duplicate data.
            action_index = out_of_order.len();
            for index in 0..out_of_order.len() {
                let stored_segment: &(SeqNumber, DemiBuffer) = &out_of_order[index];

                // Properties of the segment stored at this index.
                let stored_start: SeqNumber = stored_segment.0;
                let stored_len: u32 = stored_segment.1.len() as u32;
                debug_assert_ne!(stored_len, 0);
                let stored_end: SeqNumber = stored_start + SeqNumber::from(stored_len - 1);

                //
                // The new data segment has six possibilites when compared to an existing out-of-order segment:
                //
                //                                |<- out-of-order segment ->|
                //
                // |<- new before->|    |<- new front overlap ->|    |<- new end overlap ->|    |<- new after ->|
                //                                   |<- new duplicate ->|
                //                            |<- new completely encompassing ->|
                //
                if new_start < stored_start {
                    // The new segment starts before the start of this out-of-order segment.
                    if new_end < stored_start {
                        // The new segment comes completely before this out-of-order segment.
                        // Since the out-of-order store is sorted, we don't need to check for overlap with any more.
                        action_index = index;
                        break;
                    }
                    // The end of the new segment overlaps with the start of this out-of-order segment.
                    if stored_end < new_end {
                        // The new segment ends after the end of this out-of-order segment.  In other words, the new
                        // segment completely encompasses the out-of-order segment.

                        // Set flags to remove the currently stored segment and re-run the insertion loop, as the
                        // new segment may completely encompass even more segments.
                        another_pass_neeeded = true;
                        action_index = index;
                        break;
                    }
                    // We have some data overlap between the new segment and the front of the out-of-order segment.
                    // Trim the end of the new segment and stop checking for out-of-order overlap.
                    let excess: u32 = u32::from(new_end - stored_start) + 1;
                    new_end = new_end - SeqNumber::from(excess);
                    buf.trim(excess as usize)
                        .expect("'buf' should contain at least 'excess' bytes");
                    break;
                } else {
                    // The new segment starts at or after the start of this out-of-order segment.
                    // This is the stored_start <= new_start case.
                    if new_end <= stored_end {
                        // And the new segment ends at or before this out-of-order segment.
                        // The new segment's data is a complete duplicate of this out-of-order segment's data.
                        // Just drop the new segment.
                        return;
                    }
                    if stored_end < new_start {
                        // The new segment comes entirely after this out-of-order segment.
                        // Continue to check the next out-of-order segment for potential overlap.
                        continue;
                    }
                    // We have some data overlap between the new segment and the end of the out-of-order segment.
                    // Adjust the beginning of the new segment and continue on to check the next out-of-order segment.
                    let duplicate: u32 = u32::from(stored_end - new_start);
                    new_start = new_start + SeqNumber::from(duplicate);
                    buf.adjust(duplicate as usize)
                        .expect("'buf' should contain at least 'duplicate' bytes");
                    continue;
                }
            }

            if another_pass_neeeded {
                // The new segment completely encompassed an existing segment, which we will now remove.
                out_of_order.remove(action_index);
            }
        }

        // Insert the new segment into the correct position.
        out_of_order.insert(action_index, (new_start, buf));

        // If the out-of-order store now contains too many entries, delete the later entries.
        // ToDo: The out-of-order store is already limited (in size) by our receive window, while the below check
        // imposes a limit on the number of entries.  Do we need this?  Presumably for attack mitigation?
        while out_of_order.len() > MAX_OUT_OF_ORDER {
            out_of_order.pop_back();
        }
    }

    // This routine takes an incoming in-order TCP segment and adds the data to the user's receive queue.  If the new
    // segment fills a "hole" in the receive sequence number space allowing previously stored out-of-order data to now
    // be received, it receives that too.
    //
    // This routine also updates receive_next to reflect any data now considered "received".
    //
    // Returns true if a previously out-of-order segment containing a FIN has now been received.
    //
    pub fn receive_data(&self, seg_start: SeqNumber, buf: DemiBuffer) -> bool {
        let recv_next: SeqNumber = self.receiver.receive_next.get();

        // This routine should only be called with in-order segment data.
        debug_assert_eq!(seg_start, recv_next);

        // Push the new segment data onto the end of the receive queue.
        let mut recv_next: SeqNumber = recv_next + SeqNumber::from(buf.len() as u32);
        self.receiver.push(buf);

        // Okay, we've successfully received some new data.  Check if any of the formerly out-of-order data waiting in
        // the out-of-order queue is now in-order.  If so, we can move it to the receive queue.
        let mut added_out_of_order: bool = false;
        let mut out_of_order = self.out_of_order.borrow_mut();
        while !out_of_order.is_empty() {
            if let Some(stored_entry) = out_of_order.front() {
                if stored_entry.0 == recv_next {
                    // Move this entry's buffer from the out-of-order store to the receive queue.
                    // This data is now considered to be "received" by TCP, and included in our RCV.NXT calculation.
                    debug!("Recovering out-of-order packet at {}", recv_next);
                    if let Some(temp) = out_of_order.pop_front() {
                        recv_next = recv_next + SeqNumber::from(temp.1.len() as u32);
                        self.receiver.push(temp.1);
                        added_out_of_order = true;
                    }
                } else {
                    // Since our out-of-order list is sorted, we can stop when the next segment is not in sequence.
                    break;
                }
            }
        }

        // ToDo: Review recent change to update control block copy of recv_next upon each push to the receiver.
        // When receiving a retransmitted segment that fills a "hole" in the receive space, thus allowing a number
        // (potentially large number) of out-of-order segments to be added, we'll be modifying the TCB copy of
        // recv_next many times.
        // Anyhow that recent change removes the need for the following two lines:
        // Update our receive sequence number (i.e. RCV.NXT) appropriately.
        // self.receive_next.set(recv_next);

        // This appears to be checking if something is waiting on the receive queue, and if so, wakes that thing up.
        // Note: unlike updating receive_next (see above comment) we only do this once (i.e. outside the while loop).
        // ToDo: Verify that this is the right place and time to do this.
        if let Some(w) = self.waker.borrow_mut().take() {
            w.wake()
        }

        // This is a lot of effort just to check the FIN sequence number is correct in debug builds.
        // ToDo: Consider changing all this to "return added_out_of_order && self.out_of_order_fin.get().is_some()".
        if added_out_of_order {
            match self.out_of_order_fin.get() {
                Some(fin) => {
                    debug_assert_eq!(fin, recv_next);
                    return true;
                },
                _ => (),
            }
        }

        false
    }
}
