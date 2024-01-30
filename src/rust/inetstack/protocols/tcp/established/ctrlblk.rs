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
    collections::async_queue::{
        AsyncQueue,
        SharedAsyncQueue,
    },
    inetstack::protocols::{
        arp::SharedArpPeer,
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
        scheduler::Yielder,
        timer::SharedTimer,
        watched::SharedWatchedValue,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::std::{
    collections::VecDeque,
    convert::TryInto,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    time::{
        Duration,
        Instant,
    },
};

// TODO: Review this value (and its purpose).  It (2048 segments) of 8 KB jumbo packets would limit the unread data to
// just 16 MB.  If we don't want to lie, that is also about the max window size we should ever advertise.  Whereas TCP
// with the window scale option allows for window sizes of up to 1 GB.  This value appears to exist more because of the
// mechanism used to manage the receive queue (a VecDeque) than anything else.
const RECV_QUEUE_SZ: usize = 2048;

// TODO: Review this value (and its purpose).  It (16 segments) seems awfully small (would make fast retransmit less
// useful), and this mechanism isn't the best way to protect ourselves against deliberate out-of-order segment attacks.
// Ideally, we'd limit out-of-order data to that which (along with the unread data) will fit in the receive window.
const MAX_OUT_OF_ORDER: usize = 16;

// TCP Connection State.
// Note: This ControlBlock structure is only used after we've reached the ESTABLISHED state, so states LISTEN,
// SYN_RCVD, and SYN_SENT aren't included here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
    CloseWait,
    LastAck,
    Closed,
}

// TODO: Consider incorporating this directly into ControlBlock.
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
    reader_next: SeqNumber,

    // Sequence number of the next byte of data (or FIN) that we expect to receive.  In RFC 793 terms, this is RCV.NXT.
    receive_next: SeqNumber,

    // Receive queue.  Contains in-order received (and acknowledged) data ready for the application to read.
    recv_queue: AsyncQueue<DemiBuffer>,
}

impl Receiver {
    pub fn new(reader_next: SeqNumber, receive_next: SeqNumber) -> Self {
        Self {
            reader_next,
            receive_next,
            recv_queue: AsyncQueue::with_capacity(RECV_QUEUE_SZ),
        }
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        let buf: DemiBuffer = if let Some(size) = size {
            let mut buf: DemiBuffer = self.recv_queue.pop(&yielder).await?;
            // Split the buffer if it's too big.
            if buf.len() > size {
                buf.split_front(size)?
            } else {
                buf
            }
        } else {
            self.recv_queue.pop(&yielder).await?
        };

        self.reader_next = self.reader_next + SeqNumber::from(buf.len() as u32);

        Ok(buf)
    }

    pub fn push(&mut self, buf: DemiBuffer) {
        let buf_len: u32 = buf.len() as u32;
        self.recv_queue.push(buf);
        self.receive_next = self.receive_next + SeqNumber::from(buf_len as u32);
    }
}

/// Transmission control block for representing our TCP connection.
// TODO: Make all public fields in this structure private.
pub struct ControlBlock<N: NetworkRuntime> {
    local: SocketAddrV4,
    remote: SocketAddrV4,

    transport: N,
    #[allow(unused)]
    runtime: SharedDemiRuntime,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,

    // TODO: We shouldn't be keeping anything datalink-layer specific at this level.  The IP layer should be holding
    // this along with other remote IP information (such as routing, path MTU, etc).
    arp: SharedArpPeer<N>,

    // Send-side state information.  TODO: Consider incorporating this directly into ControlBlock.
    sender: Sender,

    // TCP Connection State.
    state: State,

    ack_delay_timeout: Duration,

    ack_deadline: SharedWatchedValue<Option<Instant>>,

    // This is our receive buffer size, which is also the maximum size of our receive window.
    // Note: The maximum possible advertised window is 1 GiB with window scaling and 64 KiB without.
    receive_buffer_size: u32,

    // TODO: Review how this is used.  We could have separate window scale factors, so there should be one for the
    // receiver and one for the sender.
    // This is the receive-side window scale factor.
    // This is the number of bits to shift to convert to/from the scaled value, and has a maximum value of 14.
    // TODO: Keep this as a u8?
    window_scale: u32,

    // Queue of out-of-order segments.  This is where we hold onto data that we've received (because it was within our
    // receive window) but can't yet present to the user because we're missing some other data that comes between this
    // and what we've already presented to the user.
    //
    out_of_order: VecDeque<(SeqNumber, DemiBuffer)>,

    // The sequence number of the FIN, if we received it out-of-order.
    // Note: This could just be a boolean to remember if we got a FIN; the sequence number is for checking correctness.
    pub out_of_order_fin: Option<SeqNumber>,

    // Receive-side state information.  TODO: Consider incorporating this directly into ControlBlock.
    receiver: Receiver,

    // Congestion control trait implementation we're currently using.
    // TODO: Consider switching this to a static implementation to avoid V-table call overhead.
    cc: Box<dyn congestion_control::CongestionControl>,

    // Current retransmission timer expiration time.
    // TODO: Consider storing this directly in the RtoCalculator.
    retransmit_deadline: SharedWatchedValue<Option<Instant>>,

    // Retransmission Timeout (RTO) calculator.
    rto_calculator: RtoCalculator,

    // Incoming packets for this connection.
    recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,

    ack_queue: SharedAsyncQueue<usize>,
}

#[derive(Clone)]
pub struct SharedControlBlock<N: NetworkRuntime>(SharedObject<ControlBlock<N>>);
//==============================================================================

impl<N: NetworkRuntime> SharedControlBlock<N> {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        runtime: SharedDemiRuntime,
        transport: N,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: SharedArpPeer<N>,
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
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        ack_queue: SharedAsyncQueue<usize>,
    ) -> Self {
        let sender: Sender = Sender::new(sender_seq_no, sender_window_size, sender_window_scale, sender_mss);
        Self(SharedObject::<ControlBlock<N>>::new(ControlBlock {
            local,
            remote,
            runtime,
            transport,
            local_link_addr,
            tcp_config,
            arp,
            sender,
            state: State::Established,
            ack_delay_timeout,
            ack_deadline: SharedWatchedValue::new(None),
            receive_buffer_size: receiver_window_size,
            window_scale: receiver_window_scale,
            out_of_order: VecDeque::new(),
            out_of_order_fin: Option::None,
            receiver: Receiver::new(receiver_seq_no, receiver_seq_no),
            cc: cc_constructor(sender_mss, sender_seq_no, congestion_control_options),
            retransmit_deadline: SharedWatchedValue::new(None),
            rto_calculator: RtoCalculator::new(),
            recv_queue,
            ack_queue,
        }))
    }

    pub fn get_local(&self) -> SocketAddrV4 {
        self.local
    }

    pub fn get_remote(&self) -> SocketAddrV4 {
        self.remote
    }

    // TODO: Remove this.  ARP doesn't belong at this layer.
    pub fn arp(&self) -> SharedArpPeer<N> {
        self.arp.clone()
    }

    pub fn send(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        let self_: Self = self.clone();
        self.sender.send(buf, self_)
    }

    pub fn retransmit(&self) {
        self.sender.retransmit(self.clone())
    }

    pub fn congestion_control_watch_retransmit_now_flag(&self) -> SharedWatchedValue<bool> {
        self.cc.get_retransmit_now_flag()
    }

    pub fn congestion_control_on_fast_retransmit(&mut self) {
        self.cc.on_fast_retransmit()
    }

    pub fn congestion_control_on_rto(&mut self, send_unacknowledged: SeqNumber) {
        self.cc.on_rto(send_unacknowledged)
    }

    pub fn congestion_control_on_send(&mut self, rto: Duration, num_sent_bytes: u32) {
        self.cc.on_send(rto, num_sent_bytes)
    }

    pub fn congestion_control_on_cwnd_check_before_send(&mut self) {
        self.cc.on_cwnd_check_before_send()
    }

    pub fn congestion_control_get_cwnd(&self) -> SharedWatchedValue<u32> {
        self.cc.get_cwnd()
    }

    pub fn congestion_control_get_limited_transmit_cwnd_increase(&self) -> SharedWatchedValue<u32> {
        self.cc.get_limited_transmit_cwnd_increase()
    }

    pub fn get_mss(&self) -> usize {
        self.sender.get_mss()
    }

    pub fn get_send_window(&self) -> SharedWatchedValue<u32> {
        self.sender.get_send_window()
    }

    pub fn get_send_unacked(&self) -> SharedWatchedValue<SeqNumber> {
        self.sender.get_send_unacked()
    }

    pub fn get_unsent_seq_no(&self) -> SharedWatchedValue<SeqNumber> {
        self.sender.get_unsent_seq_no()
    }

    pub fn get_send_next(&self) -> SharedWatchedValue<SeqNumber> {
        self.sender.get_send_next()
    }

    pub fn modify_send_next(&mut self, f: impl FnOnce(SeqNumber) -> SeqNumber) {
        self.sender.modify_send_next(f)
    }

    pub fn get_retransmit_deadline(&self) -> Option<Instant> {
        self.retransmit_deadline.get()
    }

    pub fn set_retransmit_deadline(&mut self, when: Option<Instant>) {
        self.retransmit_deadline.set(when);
    }

    pub fn watch_retransmit_deadline(&self) -> SharedWatchedValue<Option<Instant>> {
        self.retransmit_deadline.clone()
    }

    pub fn push_unacked_segment(&self, segment: UnackedSegment) {
        self.sender.push_unacked_segment(segment)
    }

    pub fn rto_add_sample(&mut self, rtt: Duration) {
        self.rto_calculator.add_sample(rtt)
    }

    pub fn rto(&self) -> Duration {
        self.rto_calculator.rto()
    }

    pub fn rto_back_off(&mut self) {
        self.rto_calculator.back_off()
    }

    pub fn unsent_top_size(&self) -> Option<usize> {
        self.sender.top_size_unsent()
    }

    pub fn pop_unsent_segment(&self, max_bytes: usize) -> Option<(DemiBuffer, bool)> {
        self.sender.pop_unsent(max_bytes)
    }

    pub fn pop_one_unsent_byte(&self) -> Option<DemiBuffer> {
        self.sender.pop_one_unsent_byte()
    }

    pub fn get_timer(&self) -> SharedTimer {
        self.runtime.get_timer()
    }

    pub fn get_now(&self) -> Instant {
        self.runtime.get_now()
    }

    pub fn receive(&mut self, ipv4_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        self.recv_queue.push((ipv4_hdr, tcp_hdr, buf));
    }

    // This is the main TCP processing routine.
    pub async fn poll(&mut self, yielder: Yielder) -> Result<!, Fail> {
        // Normal data processing in the Established state.
        loop {
            let (header, data): (TcpHeader, DemiBuffer) = match self.recv_queue.pop(&yielder).await {
                Ok((_, header, data)) if self.state == State::Established => (header, data),
                Ok(result) => {
                    self.recv_queue.push_front(result);
                    let cause: String = format!(
                        "ending receive polling loop for non-established connection (local={:?}, remote={:?})",
                        self.local, self.remote
                    );
                    error!("poll(): {}", cause);
                    return Err(Fail::new(libc::ECANCELED, &cause));
                },
                Err(e) => {
                    let cause: String = format!(
                        "ending receive polling loop for active connection (local={:?}, remote={:?})",
                        self.local, self.remote
                    );
                    warn!("poll(): {:?} ({:?})", cause, e);
                    return Err(e);
                },
            };

            debug!(
                "{:?} Connection Receiving {} bytes + {:?}",
                self.state,
                data.len(),
                header
            );

            match self.process_packet(header, data) {
                Ok(()) => (),
                Err(e) if e.errno == libc::ECONNRESET => {
                    self.state = State::CloseWait;
                    let cause: String = format!(
                        "remote closed connection, stopping processing (local={:?}, remote={:?})",
                        self.local, self.remote
                    );
                    error!("poll(): {}", cause);
                    return Err(Fail::new(libc::ECANCELED, &cause));
                },
                Err(e) => debug!("Dropped packet: {:?}", e),
            }
        }
    }

    /// This is the main function for processing an incoming packet during the Established state when the connection is
    /// active. Each step in this function return Ok if there is further processing to be done and EBADMSG if the
    /// packet should be dropped after the step.
    fn process_packet(&mut self, mut header: TcpHeader, mut data: DemiBuffer) -> Result<(), Fail> {
        let mut seg_start: SeqNumber = header.seq_num;

        let mut seg_end: SeqNumber = seg_start;
        let mut seg_len: u32 = data.len() as u32;

        // Check if the segment is in the receive window and trim off everything else.
        self.check_segment_in_window(&mut header, &mut data, &mut seg_start, &mut seg_end, &mut seg_len)?;
        self.check_rst(&header)?;
        self.check_syn(&header)?;
        self.process_ack(&header)?;

        // TODO: Check the URG bit.  If we decide to support this, how should we do it?
        if header.urg {
            warn!("Got packet with URG bit set!");
        }

        if data.len() > 0 {
            self.process_data(&mut header, data, seg_start, seg_end, seg_len)?;
        }
        self.process_remote_close(&header)?;
        // We should ACK this segment, preferably via piggybacking on a response.
        // TODO: Consider replacing the delayed ACK timer with a simple flag.
        if self.ack_deadline.get().is_none() {
            // Start the delayed ACK timer to ensure an ACK gets sent soon even if no piggyback opportunity occurs.
            let timeout: Duration = self.ack_delay_timeout;
            // Getting the current time is extremely cheap as it is just a variable lookup.
            let now: Instant = self.get_timer().now();
            self.ack_deadline.set(Some(now + timeout));
        } else {
            // We already owe our peer an ACK (the timer was already running), so cancel the timer and ACK now.
            self.ack_deadline.set(None);
            trace!("process_packet(): sending ack on deadline expiration");
            self.send_ack();
        }

        Ok(())
    }

    // Check to see if the segment is acceptable sequence-wise (i.e. contains some data that fits within the receive
    // window, or is a non-data segment with a sequence number that falls within the window).  Unacceptable segments
    // should be ACK'd (unless they are RSTs), and then dropped.
    // Returns Ok if further processing is needed and EBADMSG if the packet is not within the receive window.

    fn check_segment_in_window(
        &mut self,
        header: &mut TcpHeader,
        data: &mut DemiBuffer,
        seg_start: &mut SeqNumber,
        seg_end: &mut SeqNumber,
        seg_len: &mut u32,
    ) -> Result<(), Fail> {
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

        if header.syn {
            *seg_len += 1;
        }
        if header.fin {
            *seg_len += 1;
        }
        if *seg_len > 0 {
            *seg_end = *seg_start + SeqNumber::from(*seg_len - 1);
        }

        let receive_next: SeqNumber = self.receiver.receive_next;

        let after_receive_window: SeqNumber = receive_next + SeqNumber::from(self.get_receive_window_size());

        // Check if this segment fits in our receive window.
        // In the optimal case it starts at RCV.NXT, so we check for that first.
        if *seg_start != receive_next {
            // The start of this segment is not what we expected.  See if it comes before or after.
            if *seg_start < receive_next {
                // This segment contains duplicate data (i.e. data we've already received).
                // See if it is a complete duplicate, or if some of the data is new.
                if *seg_end < receive_next {
                    // This is an entirely duplicate (i.e. old) segment.  ACK (if not RST) and drop.
                    //
                    if !header.rst {
                        trace!("check_segment_in_window(): send ack on duplicate segment");
                        self.send_ack();
                    }
                    let cause: String = format!("duplicate packet");
                    error!("check_segment_in_window(): {}", cause);
                    return Err(Fail::new(libc::EBADMSG, &cause));
                } else {
                    // Some of this segment's data is new.  Cut the duplicate data off of the front.
                    // If there is a SYN at the start of this segment, remove it too.
                    //
                    let mut duplicate: u32 = u32::from(receive_next - *seg_start);
                    *seg_start = *seg_start + SeqNumber::from(duplicate);
                    *seg_len -= duplicate;
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
                if *seg_start >= after_receive_window {
                    // This segment is completely outside of our window.  ACK (if not RST) and drop.
                    //
                    if !header.rst {
                        trace!("check_segment_in_window(): send ack on out-of-window segment");
                        self.send_ack();
                    }
                    let cause: String = format!("packet outside of receive window");
                    error!("check_segment_in_window(): {}", cause);
                    return Err(Fail::new(libc::EBADMSG, &cause));
                }

                // At least the beginning of this segment is in the window.  We'll check the end below.
            }
        }

        // The start of the segment is in the window.
        // Check that the end of the segment is in the window, and trim it down if it is not.
        if *seg_len > 0 && *seg_end >= after_receive_window {
            let mut excess: u32 = u32::from(*seg_end - after_receive_window);
            excess += 1;
            // TODO: If we end up (after receive handling rewrite is complete) not needing seg_end and seg_len after
            // this, remove these two lines adjusting them as they're being computed needlessly.
            *seg_end = *seg_end - SeqNumber::from(excess);
            *seg_len -= excess;
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
        debug_assert!(receive_next <= *seg_start && *seg_end < after_receive_window);
        Ok(())
    }

    // Check the RST bit.
    fn check_rst(&mut self, header: &TcpHeader) -> Result<(), Fail> {
        if header.rst {
            // TODO: RFC 5961 "Blind Reset Attack Using the RST Bit" prevention would have us ACK and drop if the new
            // segment doesn't start precisely on RCV.NXT.

            // Our peer has given up.  Shut the connection down hard.
            info!("Received RST");
            // TODO: Schedule a close coroutine.
            let cause: String = format!("remote reset connection");
            info!("check_rst(): {}", cause);
            return Err(Fail::new(libc::ECONNRESET, &cause));
        }
        Ok(())
    }

    // Check the SYN bit.
    fn check_syn(&mut self, header: &TcpHeader) -> Result<(), Fail> {
        // Note: RFC 793 says to check security/compartment and precedence next, but those are largely deprecated.

        // Check the SYN bit.
        if header.syn {
            // TODO: RFC 5961 "Blind Reset Attack Using the SYN Bit" prevention would have us always ACK and drop here.

            // Receiving a SYN here is an error.
            let cause: String = format!("Received in-window SYN on established connection.");
            error!("{}", cause);
            // TODO: Send Reset.
            // TODO: Return all outstanding Receive and Send requests with "reset" responses.
            // TODO: Flush all segment queues.

            // TODO: Start the close coroutine
            return Err(Fail::new(libc::EBADMSG, &cause));
        }
        Ok(())
    }

    // Check the ACK bit.
    fn process_ack(&mut self, header: &TcpHeader) -> Result<(), Fail> {
        if !header.ack {
            // All segments on established connections should be ACKs.  Drop this segment.
            let cause: String = format!("Received non-ACK segment on established connection");
            error!("{}", cause);
            return Err(Fail::new(libc::EBADMSG, &cause));
        }

        // TODO: RFC 5961 "Blind Data Injection Attack" prevention would have us perform additional ACK validation
        // checks here.

        // Process the ACK.
        // Start by checking that the ACK acknowledges something new.
        // TODO: Look into removing Watched types.
        //
        let send_unacknowledged: SeqNumber = self.sender.get_send_unacked().get();
        let send_next: SeqNumber = self.sender.get_send_next().get();

        // TODO: Restructure this call into congestion control to either integrate it directly or make it more fine-
        // grained.  It currently duplicates the new/duplicate ack check itself internally, which is inefficient.
        // We should either make separate calls for each case or integrate those cases directly.
        let rto: Duration = self.rto_calculator.rto();
        self.cc
            .on_ack_received(rto, send_unacknowledged, send_next, header.ack_num);

        if send_unacknowledged < header.ack_num {
            if header.ack_num <= send_next {
                // Does not matter when we get this since the clock will not move between the beginning of packet
                // processing and now without a call to advance_clock.
                let now: Instant = self.get_timer().now();

                // This segment acknowledges new data (possibly and/or FIN).
                let bytes_acknowledged: u32 = (header.ack_num - send_unacknowledged).into();

                // Remove the now acknowledged data from the unacknowledged queue.
                self.sender
                    .remove_acknowledged_data(self.clone(), bytes_acknowledged, now);

                // Update SND.UNA to SEG.ACK.
                self.sender.send_unacked.set(header.ack_num);

                // Update our send window (SND.WND).
                self.sender.update_send_window(&header);

                if header.ack_num == send_next {
                    // This segment acknowledges everything we've sent so far (i.e. nothing is currently outstanding).

                    // Since we no longer have anything outstanding, we can turn off the retransmit timer.
                    self.retransmit_deadline.set(None);
                } else {
                    // Update the retransmit timer.  Some of our outstanding data is now acknowledged, but not all.
                    // TODO: This looks wrong.  We should reset the retransmit timer to match the deadline for the
                    // oldest still-outstanding data.  The below is overly generous (minor efficiency issue).
                    let deadline: Instant = now + self.rto_calculator.rto();
                    self.retransmit_deadline.set(Some(deadline));
                }

                let nbytes: usize = Into::<u32>::into(header.ack_num - send_unacknowledged) as usize;
                self.ack_queue.push(nbytes);
            } else {
                // This segment acknowledges data we have yet to send!?  Send an ACK and drop the segment.
                // TODO: See RFC 5961, this could be a Blind Data Injection Attack.
                let cause: String = format!("Received segment acknowledging data we have yet to send!");
                warn!("process_ack(): {}", cause);
                self.send_ack();
                return Err(Fail::new(libc::EBADMSG, &cause));
            }
        } else {
            // Duplicate ACK (doesn't acknowledge anything new).  We can mostly ignore this, except for fast-retransmit.
            // TODO: Implement fast-retransmit.  In which case, we'd increment our dup-ack counter here.
            warn!("process_ack(): received duplicate ack ({:?})", header.ack_num);
        }
        Ok(())
    }

    fn process_data(
        &mut self,
        header: &mut TcpHeader,
        data: DemiBuffer,
        seg_start: SeqNumber,
        mut seg_end: SeqNumber,
        mut seg_len: u32,
    ) -> Result<(), Fail> {
        // We can only process in-order data (or FIN).  Check for out-of-order segment.
        if seg_start != self.receiver.receive_next {
            debug!("Received out-of-order segment");
            // This segment is out-of-order.  If it carries data, and/or a FIN, we should store it for later processing
            // after the "hole" in the sequence number space has been filled.
            if seg_len > 0 {
                match self.state {
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
                        trace!("process_data(): send ack on out-of-order segment");
                        self.send_ack();
                    },
                    state => warn!("Ignoring data received after FIN (in state {:?}).", state),
                }
            }

            // We're done with this out-of-order segment.
            return Ok(());
        }

        // We can only legitimately receive data in ESTABLISHED, FIN-WAIT-1, and FIN-WAIT-2.
        header.fin |= self.receive_data(seg_start, data);
        Ok(())
    }

    /// Fetch a TCP header filling out various values based on our current state.
    /// TODO: Fix the "filling out various values based on our current state" part to actually do that correctly.
    pub fn tcp_header(&self) -> TcpHeader {
        let mut header: TcpHeader = TcpHeader::new(self.local.port(), self.remote.port());
        header.window_size = self.hdr_window_size();

        // Note that once we reach a synchronized state we always include a valid acknowledgement number.
        header.ack = true;
        header.ack_num = self.receiver.receive_next;

        // Return this header.
        header
    }

    /// Send an ACK to our peer, reflecting our current state.
    pub fn send_ack(&mut self) {
        let mut header: TcpHeader = self.tcp_header();

        // TODO: Think about moving this to tcp_header() as well.
        let seq_num: SeqNumber = self.get_send_next().get();
        header.seq_num = seq_num;

        // TODO: Remove this if clause once emit() is fixed to not require the remote hardware addr (this should be
        // left to the ARP layer and not exposed to TCP).
        if let Some(remote_link_addr) = self.arp().try_query(self.remote.ip().clone()) {
            self.emit(header, None, remote_link_addr);
        }
    }

    /// Transmit this message to our connected peer.
    ///
    pub fn emit(&mut self, header: TcpHeader, body: Option<DemiBuffer>, remote_link_addr: MacAddress) {
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
        // TODO: Change this to call lower levels to fill in their header information, handle routing, ARPing, etc.
        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ipv4_hdr: Ipv4Header::new(self.local.ip().clone(), self.remote.ip().clone(), IpProtocol::TCP),
            tcp_hdr: header,
            data: body,
            tx_checksum_offload: self.tcp_config.get_tx_checksum_offload(),
        };

        // Call the runtime to send the segment.
        self.transport.transmit(Box::new(segment));

        // Post-send operations follow.
        // Review: We perform these after the send, in order to keep send latency as low as possible.

        // Since we sent an ACK, cancel any outstanding delayed ACK request.
        self.set_ack_deadline(None);

        // If we sent a FIN, update our protocol state.
        if sent_fin {
            match self.state {
                // Active close.
                State::Established => self.state = State::FinWait1,
                // Passive close.
                State::CloseWait => self.state = State::LastAck,
                // We can legitimately retransmit the FIN in these states.  And we stay there until the FIN is ACK'd.
                State::FinWait1 | State::LastAck => {},
                // We shouldn't be sending a FIN from any other state.
                state => unreachable!("Sent FIN while in nonsensical TCP state {:?}", state),
            }
        }
    }

    pub fn remote_mss(&self) -> usize {
        self.sender.remote_mss()
    }

    pub fn get_ack_deadline(&self) -> SharedWatchedValue<Option<Instant>> {
        self.ack_deadline.clone()
    }

    pub fn set_ack_deadline(&mut self, when: Option<Instant>) {
        self.ack_deadline.set(when);
    }

    pub fn get_receive_window_size(&self) -> u32 {
        let bytes_unread: u32 = (self.receiver.receive_next - self.receiver.reader_next).into();
        self.receive_buffer_size - bytes_unread
    }

    fn hdr_window_size(&self) -> u16 {
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

    pub async fn push(&mut self, mut nbytes: usize, yielder: Yielder) -> Result<(), Fail> {
        loop {
            let n: usize = self.ack_queue.pop(&yielder).await?;

            if n > nbytes {
                self.ack_queue.push_front(n - nbytes);
                break Ok(());
            }

            nbytes -= n;

            if nbytes == 0 {
                break Ok(());
            }
        }
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        // TODO: Need to add a way to indicate that the other side closed (i.e. that we've received a FIN).
        // Should we do this via a zero-sized buffer?  Same as with the unsent and unacked queues on the send side?
        //
        // This code was checking for an empty receive queue by comparing sequence numbers, as in:
        //  if self.receiver.reader_next.get() == self.receiver.receive_next.get() {
        // But that will think data is available to be read once we've received a FIN, because FINs consume sequence
        // number space.  Now we call is_empty() on the receive queue instead.
        self.receiver.pop(size, yielder).await
    }

    // This routine remembers that we have received an out-of-order FIN.
    //
    fn store_out_of_order_fin(&mut self, fin: SeqNumber) {
        self.out_of_order_fin = Some(fin);
    }

    // This routine takes an incoming TCP segment and adds it to the out-of-order receive queue.
    // If the new segment had a FIN it has been removed prior to this routine being called.
    // Note: Since this is not the "fast path", this is written for clarity over efficiency.
    //
    fn store_out_of_order_segment(&mut self, mut new_start: SeqNumber, mut new_end: SeqNumber, mut buf: DemiBuffer) {
        let mut action_index: usize = self.out_of_order.len();
        let mut another_pass_neeeded: bool = true;

        while another_pass_neeeded {
            another_pass_neeeded = false;

            // Find the new segment's place in the out-of-order store.
            // The out-of-order store is sorted by starting sequence number, and contains no duplicate data.
            action_index = self.out_of_order.len();
            for index in 0..self.out_of_order.len() {
                let stored_segment: &(SeqNumber, DemiBuffer) = &self.out_of_order[index];

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
                self.out_of_order.remove(action_index);
            }
        }

        // Insert the new segment into the correct position.
        self.out_of_order.insert(action_index, (new_start, buf));

        // If the out-of-order store now contains too many entries, delete the later entries.
        // TODO: The out-of-order store is already limited (in size) by our receive window, while the below check
        // imposes a limit on the number of entries.  Do we need this?  Presumably for attack mitigation?
        while self.out_of_order.len() > MAX_OUT_OF_ORDER {
            self.out_of_order.pop_back();
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
    fn receive_data(&mut self, seg_start: SeqNumber, buf: DemiBuffer) -> bool {
        let recv_next: SeqNumber = self.receiver.receive_next;

        // This routine should only be called with in-order segment data.
        debug_assert_eq!(seg_start, recv_next);

        // Push the new segment data onto the end of the receive queue.
        let mut recv_next: SeqNumber = recv_next + SeqNumber::from(buf.len() as u32);
        // This inserts the segment and wakes a waiting pop coroutine.
        self.receiver.push(buf);

        // Okay, we've successfully received some new data.  Check if any of the formerly out-of-order data waiting in
        // the out-of-order queue is now in-order.  If so, we can move it to the receive queue.
        let mut added_out_of_order: bool = false;
        while !self.out_of_order.is_empty() {
            if let Some(stored_entry) = self.out_of_order.front() {
                if stored_entry.0 == recv_next {
                    // Move this entry's buffer from the out-of-order store to the receive queue.
                    // This data is now considered to be "received" by TCP, and included in our RCV.NXT calculation.
                    debug!("Recovering out-of-order packet at {}", recv_next);
                    if let Some(temp) = self.out_of_order.pop_front() {
                        recv_next = recv_next + SeqNumber::from(temp.1.len() as u32);
                        // This inserts the segment and wakes a waiting pop coroutine.
                        self.receiver.push(temp.1);
                        added_out_of_order = true;
                    }
                } else {
                    // Since our out-of-order list is sorted, we can stop when the next segment is not in sequence.
                    break;
                }
            }
        }

        // TODO: Review recent change to update control block copy of recv_next upon each push to the receiver.
        // When receiving a retransmitted segment that fills a "hole" in the receive space, thus allowing a number
        // (potentially large number) of out-of-order segments to be added, we'll be modifying the TCB copy of
        // recv_next many times.
        // Anyhow that recent change removes the need for the following two lines:
        // Update our receive sequence number (i.e. RCV.NXT) appropriately.
        // self.receive_next.set(recv_next);

        // This is a lot of effort just to check the FIN sequence number is correct in debug builds.
        // TODO: Consider changing all this to "return added_out_of_order && self.out_of_order_fin.get().is_some()".
        if added_out_of_order {
            match self.out_of_order_fin {
                Some(fin) => {
                    debug_assert_eq!(fin, recv_next);
                    return true;
                },
                _ => (),
            }
        }

        false
    }

    fn process_remote_close(&mut self, header: &TcpHeader) -> Result<(), Fail> {
        if header.fin {
            trace!("Received FIN");
            // 2. Push empty buffer to indicate EOF.
            // TODO: set err bit and wake.
            self.receiver.push(DemiBuffer::new(0));

            // 3. Advance RCV.NXT over the FIN.
            self.receiver.receive_next = self.receiver.receive_next + SeqNumber::from(1);

            // 4. Since we consumed the FIN we ACK immediately rather than opportunistically.
            // TODO: Consider doing this opportunistically.  Note our current tests expect the immediate behavior.
            trace!("process_remote_close(): send ack on received fin");
            self.send_ack();
            let cause: String = format!("connection received FIN");
            info!("process_remote_close(): {}", cause);
            return Err(Fail::new(libc::ECONNRESET, &cause));
        }

        Ok(())
    }

    /// Send a fin by pushing a zero-length DemiBuffer to the sender function.
    fn send_fin(&mut self) {
        // Construct FIN.
        let fin_buf: DemiBuffer = DemiBuffer::new(0);
        // Send.
        if let Err(e) = self.send(fin_buf) {
            warn!("send_fin(): failed to send fin ({:?})", e);
        }
    }

    // This coroutine runs the close protocol.
    pub async fn close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        // Assert we are in a valid state and move to new state.
        match self.state {
            State::Established => self.local_close(yielder).await,
            State::CloseWait => self.remote_already_closed(yielder).await,
            _ => {
                let cause: String = format!("socket is already closing");
                error!("close(): {}", cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    async fn local_close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        // 0. Set state.
        self.state = State::FinWait1;
        // 1. Send FIN.
        self.send_fin();

        while self.state != State::TimeWait {
            // Wait for next packet.
            let (_, header, _) = self.recv_queue.pop(&yielder).await?;

            // Check ACK.
            self.state = match self.process_ack(&header) {
                // Got ACK to our FIN.
                Ok(()) => match self.state {
                    State::FinWait1 => State::FinWait2,
                    State::FinWait2 => State::FinWait2,
                    State::Closing => State::TimeWait,
                    state => unreachable!("Cannot be in any other state at this point: {:?}", state),
                },
                // Don't do anything if this is an unexpected message.
                Err(_) => self.state,
            };

            // TODO: Receive data in the FINWAIT-1 and FINWAIT-2 states.

            // Check FIN.
            self.state = match self.process_remote_close(&header) {
                // No FIN, keep waiting.
                Ok(()) => self.state,
                // Found FIN, move to next state.
                Err(e) if e.errno == libc::ECONNRESET => match self.state {
                    State::FinWait1 => State::Closing,
                    State::FinWait2 => State::TimeWait,
                    state => unreachable!("Cannot be in any other state: {:?}", state),
                },
                // Some other error, stop close protocol.
                Err(e) => return Err(e),
            }
        }

        // TODO: Get 2MSL value or linger option if set.
        // TODO: Turn this on when our tests support it.
        // let timeout_yielder: Yielder = Yielder::new();
        // self.runtime
        //     .get_timer()
        //     .wait(Duration::from_secs(1), &timeout_yielder)
        //     .await?;

        self.state = State::Closed;
        Ok(())
    }

    async fn remote_already_closed(&mut self, yielder: Yielder) -> Result<(), Fail> {
        // 0. Set state.
        self.state = State::LastAck;
        // 1. Send FIN.
        self.send_fin();
        // Wait for ACK of FIN.
        loop {
            // Wait for next packet.
            let (_, header, _) = self.recv_queue.pop(&yielder).await?;

            // Check ACK.
            match self.process_ack(&header) {
                Ok(()) => break,
                Err(_) => (),
            }
        }
        Ok(())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<N: NetworkRuntime> Deref for SharedControlBlock<N> {
    type Target = ControlBlock<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedControlBlock<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
