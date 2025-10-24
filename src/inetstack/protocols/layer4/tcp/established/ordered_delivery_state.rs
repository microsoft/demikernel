use std::{
    cmp,
    collections::VecDeque,
    time::{Duration, Instant},
};

use arrayvec::ArrayVec;
use futures::{never::Never, pin_mut, select_biased, FutureExt};

use crate::{
    collections::{
        async_queue::{AsyncQueue, SharedAsyncQueue},
        async_value::SharedAsyncValue,
    },
    expect_ok,
    inetstack::{
        consts::{MAX_BATCH_SIZE_NUM_PACKETS, MAX_HEADER_SIZE},
        protocols::{
            layer3::SharedLayer3Endpoint,
            layer4::tcp::{
                established::{
                    ctrlblk::{ControlBlock, State},
                    MAX_WINDOW_SIZE_WITHOUT_SCALING, MAX_WINDOW_SIZE_WITH_SCALING,
                },
                header::TcpHeader,
                SeqNumber,
            },
        },
    },
    runtime::{conditional_yield_until, fail::Fail, memory::DemiBuffer, SharedDemiRuntime},
};

//======================================================================================================================
// Data Structures
//======================================================================================================================

// Structure of entries on our unacknowledged queue.
// TODO: We currently allocate these on the fly when we add a buffer to the queue.  Would be more efficient to have a
// buffer structure that held everything we need directly, thus avoiding this extra wrapper.
//
struct UnackedSegment {
    pub bytes: Option<DemiBuffer>,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

//======================================================================================================================
// Constants
//======================================================================================================================

// TODO: Review this value (and its purpose).  It (16 segments) seems awfully small (would make fast retransmit less
// useful), and this mechanism isn't the best way to protect ourselves against deliberate out-of-order segment attacks.
// Ideally, we'd limit out-of-order data to that which (along with the unread data) will fit in the receive window.
const MAX_OUT_OF_ORDER_SIZE_FRAMES: usize = 1024;

// Hard limit for unsent queue.
// TODO: Remove this.  We should limit the unsent queue by either having a (configurable) send buffer size (in bytes,
// not segments) and rejecting send requests that exceed that, or by limiting the user's send buffer allocations.
const UNSENT_QUEUE_CUTOFF: usize = 1024;

// Minimum size for unacknowledged queue. This number doesn't really matter very much, it just sets the initial size
// of the unacked queue, below which memory allocation is not required.
const MIN_UNACKED_QUEUE_SIZE_FRAMES: usize = 64;

// Minimum size for unsent queue. This number doesn't really matter very much, it just sets the initial size
// of the unacked queue, below which memory allocation is not required.
const MIN_UNSENT_QUEUE_SIZE_FRAMES: usize = 64;

pub struct OrderedDeliveryState {
    //
    // Send Sequence Space:
    //
    //                     |<-----------------send window size----------------->|
    //                     |                                                    |
    //                send_unacked               send_next         send_unacked + send window
    //                     v                         v                          v
    // ... ----------------|-------------------------|--------------------------|--------------------------------
    //       acknowledged  |      unacknowledged     |     allowed to send      |  future sequence number space
    //
    // Note: In RFC 793 terminology, send_unacked is SND.UNA, send_next is SND.NXT, and "send window" is SND.WND.
    //

    // Sequence Number of the oldest byte of unacknowledged sent data.  In RFC 793 terms, this is SND.UNA.
    send_unacked: SharedAsyncValue<SeqNumber>,

    // Queue of unacknowledged sent data.  RFC 793 calls this the "retransmission queue".
    unacked_queue: SharedAsyncQueue<UnackedSegment>,

    // Send timers
    // Current retransmission timer expiration time.
    // TODO: Consider storing this directly in the RtoCalculator.
    retransmit_deadline_time_secs: SharedAsyncValue<Option<Instant>>,

    // In RFC 793 terms, this is SND.NXT.
    pub send_next_seq_no: SharedAsyncValue<SeqNumber>,

    // Sequence number of next data to be pushed but not sent. When there is an open window, this is equivalent to
    // send_next_seq_no.
    unsent_next_seq_no: SeqNumber,

    // Sequence number of the FIN, after we should never allocate more sequence numbers.
    sender_fin_seq_no: Option<SeqNumber>,

    // This is the send buffer (user data we do not yet have window to send). If the option is None, then it indicates
    // a FIN. This keeps us from having to allocate an empty Demibuffer to indicate FIN.
    unsent_queue: SharedAsyncQueue<DemiBuffer>,

    //
    // Receive Sequence Space:
    //
    //                     |<---------------receive_buffer_size---------------->|
    //                     |                                                    |
    //                     |                         |<-----receive window----->|
    //                 read_next               receive_next       receive_next + receive window
    //                     v                         v                          v
    // ... ----------------|-------------------------|--------------------------|------------------------------
    //      read by user   |  received but not read  |    willing to receive    | future sequence number space
    //
    // Note: In RFC 793 terminology, receive_next is RCV.NXT, and "receive window" is RCV.WND.
    //

    // Sequence number of next byte of data in the unread queue.
    reader_next_seq_no: SeqNumber,

    // Sequence number of the next byte of data (or FIN) that we expect to receive.  In RFC 793 terms, this is RCV.NXT.
    pub receive_next_seq_no: SeqNumber,

    // Sequnce number of the last byte of data (FIN).
    recv_fin_seq_no: SharedAsyncValue<Option<SeqNumber>>,

    // Pop queue.  Contains in-order received (and acknowledged) data ready for the application to read.
    pop_queue: AsyncQueue<DemiBuffer>,

    // The amount of time before we will send a bare ACK.
    ack_delay_timeout_secs: Duration,
    // The deadline when we will send a bare ACK if there are no outgoing packets by then.
    pub ack_deadline_time_secs: SharedAsyncValue<Option<Instant>>,

    // This is our receive buffer size, which is also the maximum size of our receive window.
    // Note: The maximum possible advertised window is 1 GiB with window scaling and 64 KiB without.
    buffer_size_bytes: u32,

    // This is the number of bits to shift to convert to/from the scaled value, and has a maximum value of 14.
    window_scale_shift_bits: u8,

    // Queue of out-of-order segments.  This is where we hold onto data that we've received (because it was within our
    // receive window) but can't yet present to the user because we're missing some other data that comes between this
    // and what we've already presented to the user.
    out_of_order_frames: VecDeque<(SeqNumber, DemiBuffer)>,
}

impl OrderedDeliveryState {
    pub fn new(
        // Required to initialize sender variables
        local_seq_no: SeqNumber,
        // Required to initialize receiver variables
        reader_next_seq_no: SeqNumber,
        receive_next_seq_no: SeqNumber,
        ack_delay_timeout_secs: Duration,
        window_size_bytes: u32,
        window_scale_shift_bits: u8,
    ) -> Self {
        Self {
            // Sender variables
            send_unacked: SharedAsyncValue::new(local_seq_no),
            unacked_queue: SharedAsyncQueue::with_capacity(MIN_UNACKED_QUEUE_SIZE_FRAMES),
            retransmit_deadline_time_secs: SharedAsyncValue::new(None),
            send_next_seq_no: SharedAsyncValue::new(local_seq_no),
            unsent_next_seq_no: local_seq_no,
            sender_fin_seq_no: None,
            unsent_queue: SharedAsyncQueue::with_capacity(MIN_UNSENT_QUEUE_SIZE_FRAMES),

            // Receiver variables
            reader_next_seq_no,
            receive_next_seq_no,
            recv_fin_seq_no: SharedAsyncValue::new(None),
            pop_queue: AsyncQueue::with_capacity(1024),
            ack_delay_timeout_secs,
            ack_deadline_time_secs: SharedAsyncValue::new(None),
            buffer_size_bytes: window_size_bytes,
            window_scale_shift_bits,
            out_of_order_frames: VecDeque::with_capacity(64),
        }
    }

    //======================================================================================================================
    // Sender methods
    //======================================================================================================================

    fn process_acked_fin(cb: &mut ControlBlock, bytes_remaining: usize, ack_num: SeqNumber) -> usize {
        // This buffer is the end-of-send marker.  So we should only have one byte of acknowledged
        // sequence space remaining (corresponding to our FIN).
        debug_assert_eq!(bytes_remaining, 1);

        // Double check that the ack is for the FIN sequence number.
        debug_assert_eq!(
            ack_num,
            cb.delivery
                .sender_fin_seq_no
                .map(|s| { s + 1.into() })
                .expect("should have a FIN set")
        );

        cb.connection_management.state = match cb.connection_management.state {
            State::FinWait1 => State::FinWait2,
            State::Closing => State::TimeWait,
            State::LastAck => State::Closed,
            state => unreachable!(
                "cannot receive a response to a FIN if one was not sent in state {:?}",
                state
            ),
        };

        0
    }

    fn process_acked_segment(&mut self, bytes_remaining: usize, mut segment: UnackedSegment) -> usize {
        let mut data = segment
            .bytes
            .take()
            .expect("there should be data because this is not a FIN.");
        if data.len() > bytes_remaining {
            // Put this segment on the unacknowledged list.
            let unacked_segment = UnackedSegment {
                bytes: Some(
                    data.split_back(bytes_remaining)
                        .expect("Should be able to split back because we just checked the length"),
                ),
                initial_tx: None,
            };
            // Leave this segment on the unacknowledged queue.
            self.unacked_queue.push_front(unacked_segment);
            0
        } else {
            bytes_remaining - data.len()
        }
    }

    fn update_retransmit_deadline(&mut self, now: Instant, rto: Duration) -> Option<Instant> {
        match self.unacked_queue.front() {
            Some(UnackedSegment {
                bytes: _,
                initial_tx: Some(initial_tx),
            }) => Some(*initial_tx + rto),
            Some(UnackedSegment {
                bytes: _,
                initial_tx: None,
            }) => Some(now + rto),
            None => None,
        }
    }

    // This function sends a list of packets (or FIN if empty) and waits for it to be acked.
    pub async fn push(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        runtime: &mut SharedDemiRuntime,
        bufs: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>,
    ) -> Result<(), Fail> {
        // If the user is done sending (i.e. has called close on this connection), then they shouldn't be sending.
        debug_assert!(cb.delivery.sender_fin_seq_no.is_none());

        // TODO: We need to fix this the correct way: limit our send buffer size to the amount we're willing to buffer.
        if cb.delivery.unsent_queue.len() > UNSENT_QUEUE_CUTOFF - 1 {
            return Err(Fail::new(libc::EBUSY, "too many packets to send"));
        }

        trace!("push(): total unsent segments={:?}", cb.delivery.unsent_queue.len());

        // Check if closing the socket and sending FIN.
        if bufs.is_empty() {
            // We can always send the FIN immediately.
            cb.delivery.sender_fin_seq_no = Some(cb.delivery.unsent_next_seq_no);
            cb.delivery.unsent_next_seq_no = cb.delivery.unsent_next_seq_no + 1.into();
            Self::send_fin(cb, layer3_endpoint, runtime.now())?;
        } else {
            for mut buf in bufs.into_iter() {
                cb.delivery.unsent_next_seq_no = cb.delivery.unsent_next_seq_no + (buf.len() as u32).into();
                if cb.flow_control.send_window.get() > 0 {
                    Self::send_segment(cb, layer3_endpoint, runtime.now(), &mut buf);

                    if !buf.is_empty() {
                        cb.delivery.unsent_queue.push(buf);
                    }
                }
            }
        }

        if !cb.delivery.unacked_queue.is_empty() {
            trace!("push(): total unacked segments={:?}", cb.delivery.unacked_queue.len());
        }

        // Wait until the sequnce number of the pushed buffer is acknowledged.
        let mut send_unacked_watched = cb.delivery.send_unacked.clone();
        let ack_seq_no = cb.delivery.unsent_next_seq_no;
        debug_assert!(send_unacked_watched.get() < ack_seq_no);
        while send_unacked_watched.get() < ack_seq_no {
            send_unacked_watched.wait_for_change(None).await?;
        }
        Ok(())
    }

    pub async fn background_sender(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        runtime: &mut SharedDemiRuntime,
    ) -> Result<Never, Fail> {
        loop {
            // Get next bit of unsent data.
            let buffer = cb.delivery.unsent_queue.pop(None).await?;
            Self::send_buffer(cb, layer3_endpoint, runtime.now(), buffer).await?;
        }
    }

    fn send_fin(cb: &mut ControlBlock, layer3_endpoint: &mut SharedLayer3Endpoint, now: Instant) -> Result<(), Fail> {
        debug_assert!(cb.delivery.sender_fin_seq_no.is_some());

        let mut header = Self::tcp_header(cb, cb.delivery.sender_fin_seq_no);
        header.fin = true;
        Self::emit(cb, layer3_endpoint, header, None);
        // Update SND.NXT.
        cb.delivery.send_next_seq_no.modify(|s| s + 1.into());

        // Add the FIN to our unacknowledged queue.
        let unacked_segment = UnackedSegment {
            bytes: None,
            initial_tx: Some(now),
        };
        cb.delivery.unacked_queue.push(unacked_segment);
        // Set the retransmit timer.
        if cb.delivery.retransmit_deadline_time_secs.get().is_none() {
            let rto = cb.congestion_control.rto_calculator.rto();
            cb.delivery.retransmit_deadline_time_secs.set(Some(now + rto));
        }
        Ok(())
    }

    async fn send_buffer(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        now: Instant,
        mut buffer: DemiBuffer,
    ) -> Result<(), Fail> {
        let mut send_unacked_watched = cb.delivery.send_unacked.clone();
        let mut cwnd_watched = cb.congestion_control.cc_algorithm.get_cwnd();

        // The limited transmit algorithm may increase the effective size of cwnd by up to 2 * mss.
        let mut ltci_watched = cb.congestion_control.cc_algorithm.get_limited_transmit_cwnd_increase();
        let mut win_sz_watched = cb.flow_control.send_window.clone();

        // Try in a loop until we send this segment.
        loop {
            // If we don't have any window size at all, we need to transition to PERSIST mode and
            // repeatedly send window probes until window opens up.
            if win_sz_watched.get() == 0 {
                // Send a window probe (this is a one-byte packet designed to elicit a window update from our peer).
                Self::send_window_probe(cb, layer3_endpoint, now, buffer.split_front(1)?).await?;
            } else {
                // TODO: Nagle's algorithm - We need to coalese small buffers together to send MSS sized packets.
                // TODO: Silly window syndrome - See RFC 1122's discussion of the SWS avoidance algorithm.

                // We have some window, try to send some or all of the segment.
                let _ = Self::send_segment(cb, layer3_endpoint, now, &mut buffer);
                // If the buffer is now empty, then we sent all of it.
                if buffer.is_empty() {
                    return Ok(());
                }
                // Otherwise, wait until something limiting the window changes and then try again to finish sending
                // the segment.
                futures::select_biased! {
                    _ = send_unacked_watched.wait_for_change(None).fuse() => (),
                    _ = cb.delivery.send_next_seq_no.wait_for_change(None).fuse() => (),
                    _ = win_sz_watched.wait_for_change(None).fuse() => (),
                    _ = cwnd_watched.wait_for_change(None).fuse() => (),
                    _ = ltci_watched.wait_for_change(None).fuse() => (),
                };
            }
        }
    }

    async fn send_window_probe(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        now: Instant,
        probe: DemiBuffer,
    ) -> Result<(), Fail> {
        // Update SND.NXT.
        cb.delivery.send_next_seq_no.modify(|s| s + SeqNumber::from(1));

        // Add the probe byte (as a new separate buffer) to our unacknowledged queue.
        let unacked_segment = UnackedSegment {
            bytes: Some(probe.clone()),
            initial_tx: Some(now),
        };
        cb.delivery.unacked_queue.push(unacked_segment);

        // Note that we loop here *forever*, exponentially backing off.
        // TODO: Use the correct PERSIST mode timer here.
        let mut timeout = Duration::from_secs(1);
        let mut win_sz_watched = cb.flow_control.send_window.clone();
        loop {
            // Create packet.
            let header = Self::tcp_header(cb, None);
            Self::emit(cb, layer3_endpoint, header, Some(probe.clone()));

            match win_sz_watched.wait_for_change(Some(timeout)).await {
                Ok(_) => return Ok(()),
                Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => timeout *= 2,
                Err(_) => {
                    unreachable!(
                        "either the ack deadline changed or the deadline passed, no other errors are possible!"
                    )
                },
            }
        }
    }

    // Takes a segment and attempts to send it. The buffer must be non-zero length and the function returns the number
    // of bytes sent.
    fn send_segment(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        now: Instant,
        segment: &mut DemiBuffer,
    ) -> usize {
        debug_assert!(!segment.is_empty());

        let max_frame_size_bytes = Self::get_open_window_size_bytes(cb);
        if max_frame_size_bytes == 0 {
            return 0;
        }

        // Split the packet if necessary.
        // TODO: Use a scatter/gather array to coalesce multiple buffers into a single segment.
        let (frame_size_bytes, do_push) = {
            if segment.len() > max_frame_size_bytes {
                // Suppress PSH flag for partial buffers.
                (max_frame_size_bytes, false)
            } else {
                // We can just send the whole packet. Clone it so we can attach headers/retransmit it later.
                (segment.len(), true)
            }
        };
        let segment_data = segment
            .split_front(frame_size_bytes)
            .expect("Should be able to split within the length of the buffer");

        let segment_data_len = segment_data.len() as u32;

        let rto = cb.congestion_control.rto_calculator.rto();
        cb.congestion_control.cc_algorithm.on_send(
            rto,
            (cb.delivery.send_next_seq_no.get() - cb.delivery.send_unacked.get()).into(),
        );

        // Prepare the segment and send it.
        let mut header = Self::tcp_header(cb, None);
        if do_push {
            header.psh = true;
        }
        Self::emit(cb, layer3_endpoint, header, Some(segment_data.clone()));

        // Update SND.NXT.
        cb.delivery
            .send_next_seq_no
            .modify(|s| s + SeqNumber::from(segment_data_len));

        // Put this segment on the unacknowledged list.
        let unacked_segment = UnackedSegment {
            bytes: Some(segment_data),
            initial_tx: Some(now),
        };

        if !cb.delivery.unacked_queue.is_empty() {
            trace!(
                "send_segment(): unacked_queue.len() = {:?}",
                cb.delivery.unacked_queue.len()
            );
        }

        cb.delivery.unacked_queue.push(unacked_segment);

        // Set the retransmit timer.
        if cb.delivery.retransmit_deadline_time_secs.get().is_none() {
            let rto = cb.congestion_control.rto_calculator.rto();
            cb.delivery.retransmit_deadline_time_secs.set(Some(now + rto));
        }
        segment_data_len as usize
    }

    /// Fetch a TCP header filling out various values based on our current state.
    /// If a sequence number is provided, use it otherwise, use the current unsent sequence number.
    /// The only time that the unsent sequence number is not used is when we are retransmitting.
    pub fn tcp_header(cb: &mut ControlBlock, seq_num: Option<SeqNumber>) -> TcpHeader {
        let mut header = TcpHeader::new(
            cb.connection_management.local.port(),
            cb.connection_management.remote.port(),
        );
        header.window_size = cb.delivery.hdr_window_size();

        // Note that once we reach a synchronized state we always include a valid acknowledgement number.
        header.ack = true;
        header.ack_num = cb.delivery.receive_next_seq_no;
        header.seq_num = seq_num.unwrap_or(cb.delivery.send_next_seq_no.get());

        header
    }

    fn get_open_window_size_bytes(cb: &mut ControlBlock) -> usize {
        // Calculate amount of data in flight (SND.NXT - SND.UNA).
        let send_unacknowledged = cb.delivery.send_unacked.get();
        let send_next = cb.delivery.send_next_seq_no.get();
        let sent_data = (send_next - send_unacknowledged).into();

        // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
        cb.congestion_control.cc_algorithm.on_cwnd_check_before_send();
        let cwnd = cb.congestion_control.cc_algorithm.get_cwnd();

        // The limited transmit algorithm can increase the effective size of cwnd by up to 2MSS.
        let effective_cwnd = cwnd.get()
            + cb.congestion_control
                .cc_algorithm
                .get_limited_transmit_cwnd_increase()
                .get();

        let win_sz = cb.flow_control.send_window.get();

        if Self::has_open_window(win_sz, sent_data, effective_cwnd) {
            Self::calculate_open_window_bytes(win_sz, sent_data, cb.flow_control.mss, effective_cwnd)
        } else {
            0
        }
    }

    fn has_open_window(win_sz: u32, sent_data: u32, effective_cwnd: u32) -> bool {
        win_sz > 0 && win_sz >= sent_data && effective_cwnd >= sent_data
    }

    fn calculate_open_window_bytes(win_sz: u32, sent_data: u32, mss: usize, effective_cwnd: u32) -> usize {
        cmp::min(
            cmp::min((win_sz - sent_data) as usize, mss),
            (effective_cwnd - sent_data) as usize,
        )
    }

    pub async fn background_retransmitter(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        runtime: &mut SharedDemiRuntime,
    ) -> Result<Never, Fail> {
        // Watch the retransmission deadline.
        let mut rtx_deadline_watched = cb.delivery.retransmit_deadline_time_secs.clone();
        // Watch the fast retransmit flag.
        let mut rtx_fast_retransmit_watched = cb.congestion_control.cc_algorithm.get_retransmit_now_flag();
        loop {
            let rtx_deadline = rtx_deadline_watched.get();
            let rtx_fast_retransmit = rtx_fast_retransmit_watched.get();
            if rtx_fast_retransmit {
                // Notify congestion control about fast retransmit.
                cb.congestion_control.cc_algorithm.on_fast_retransmit();

                // Retransmit earliest unacknowledged segment.
                Self::retransmit(cb, layer3_endpoint);
                continue;
            }

            // If either changed, wake up.
            let something_changed = async {
                select_biased!(
                    _ = rtx_deadline_watched.wait_for_change(None).fuse() => (),
                    _ = rtx_fast_retransmit_watched.wait_for_change(None).fuse() => (),
                )
            };
            pin_mut!(something_changed);
            match conditional_yield_until(something_changed, rtx_deadline).await {
                Ok(()) => match cb.delivery.sender_fin_seq_no {
                    Some(fin_seq_no) if cb.delivery.send_unacked.get() > fin_seq_no => {
                        return Err(Fail::new(libc::ECONNRESET, "connection closed"));
                    },
                    _ => continue,
                },
                Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => {
                    // Retransmit timeout.
                    // Notify congestion control about RTO.
                    cb.congestion_control
                        .cc_algorithm
                        .on_rto(cb.delivery.send_unacked.get());

                    // RFC 6298 Section 5.4: Retransmit earliest unacknowledged segment.
                    Self::retransmit(cb, layer3_endpoint);

                    // RFC 6298 Section 5.5: Back off the retransmission timer.
                    cb.congestion_control.rto_calculator.back_off();

                    // RFC 6298 Section 5.6: Restart the retransmission timer with the new RTO.
                    let deadline = runtime.now() + cb.congestion_control.rto_calculator.rto();
                    cb.delivery.retransmit_deadline_time_secs.set(Some(deadline));
                },
                Err(_) => {
                    unreachable!(
                        "either the retransmit deadline changed or the deadline passed, no other errors are possible!"
                    )
                },
            }
        }
    }

    /// Retransmits the earliest segment that has not (yet) been acknowledged by our peer.
    pub fn retransmit(cb: &mut ControlBlock, layer3_endpoint: &mut SharedLayer3Endpoint) {
        if let Some(segment) = cb.delivery.unacked_queue.front_mut() {
            // We're retransmitting this, so we can no longer use an ACK for it as an RTT measurement (as we can't tell
            // if the ACK is for the original or the retransmission).  Remove the transmission timestamp from the entry.
            segment.initial_tx.take();

            // Clone the segment data here for retransmission.
            let data = segment.bytes.clone();

            // TODO: Issue #198 Repacketization - we should send a full MSS (and set the FIN flag if applicable).

            let mut header = Self::tcp_header(cb, Some(cb.delivery.send_unacked.get()));

            if data.is_some() {
                // Regular packet, so set the PSH flag.
                header.psh = true;
            } else {
                // If there is no data, then its a FIN.
                header.fin = true;
            }

            Self::emit(cb, layer3_endpoint, header, data);
        }
    }

    pub fn process_ack(cb: &mut ControlBlock, header: &TcpHeader, now: Instant) {
        // Start by checking that the ACK acknowledges something new.
        let send_unacknowledged = cb.delivery.send_unacked.get();
        // Check and update send window if necessary.
        cb.flow_control.update_send_window(header);

        if send_unacknowledged < header.ack_num {
            // Remove the now acknowledged data from the unacknowledged queue, update the acked sequence number
            // and update the sender window.

            // Convert the difference in sequence numbers into a u32.
            let bytes_acknowledged: u32 = (header.ack_num - cb.delivery.send_unacked.get()).into();
            // Convert that into a usize for counting bytes to remove from the unacked queue.
            let mut bytes_remaining = bytes_acknowledged as usize;
            // Remove bytes from the unacked queue.
            while bytes_remaining != 0 {
                bytes_remaining = match cb.delivery.unacked_queue.try_pop() {
                    Some(segment) if segment.bytes.is_none() => {
                        Self::process_acked_fin(cb, bytes_remaining, header.ack_num)
                    },
                    Some(segment) => {
                        // We add the sample outside the Sender function to separate state.
                        cb.congestion_control.add_sample(segment.initial_tx, now);
                        cb.delivery.process_acked_segment(bytes_remaining, segment)
                    },
                    None => {
                        unreachable!("There should be enough data in the unacked_queue for the number of bytes acked")
                    }, // Shouldn't have bytes_remaining with no segments remaining in unacked_queue.
                };
            }

            // Update SND.UNA to SEG.ACK.
            cb.delivery.send_unacked.set(header.ack_num);

            // Reset the retransmit timer if necessary. If there is more data that hasn't been acked, then set to the
            // next segment deadline, otherwise, do not set.
            let retransmit_deadline_time_secs = cb
                .delivery
                .update_retransmit_deadline(now, cb.congestion_control.rto_calculator.rto());
            #[cfg(debug_assertions)]
            if retransmit_deadline_time_secs.is_none() {
                debug_assert_eq!(cb.delivery.send_next_seq_no.get(), header.ack_num);
            }
            cb.delivery
                .retransmit_deadline_time_secs
                .set(retransmit_deadline_time_secs);
        } else {
            // Duplicate ACK (doesn't acknowledge anything new).  We can mostly ignore this, except for fast-retransmit.
            // TODO: Implement fast-retransmit.  In which case, we'd increment our dup-ack counter here.
            trace!(
                "process_ack(): received duplicate ack ({:?}); unacked len = {:?}",
                header.ack_num,
                cb.delivery.unacked_queue.len()
            );
        }
    }

    /// Send an ACK to our peer, reflecting our current state.
    pub fn send_ack(cb: &mut ControlBlock, layer3_endpoint: &mut SharedLayer3Endpoint) {
        let header = Self::tcp_header(cb, None);
        Self::emit(cb, layer3_endpoint, header, None);
    }

    /// Transmit this message to our connected peer.
    pub fn emit(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        header: TcpHeader,
        body: Option<DemiBuffer>,
    ) {
        // Only perform this debug print in debug builds.  debug_assertions is compiler set in non-optimized builds.
        let mut pkt = match body {
            Some(body) => {
                debug!(
                    "L4 OUTGOING {:?} Connection sending {} bytes + {:?}",
                    cb.connection_management.state,
                    body.len(),
                    header
                );
                body
            },
            _ => {
                debug!(
                    "L4 OUTGOING {:?} Connection sending 0 bytes + {:?}",
                    cb.connection_management.state, header
                );
                DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16)
            },
        };

        // This routine should only ever be called to send TCP segments that contain a valid ACK value.
        debug_assert!(header.ack);

        let remote_ipv4_addr = *cb.connection_management.remote.ip();
        header.serialize_and_attach(
            &mut pkt,
            cb.connection_management.local.ip(),
            cb.connection_management.remote.ip(),
            cb.connection_management.tcp_config.get_tx_checksum_offload(),
        );

        // Call lower L3 layer to send the segment.
        if let Err(e) = layer3_endpoint.transmit_tcp_packet_nonblocking(remote_ipv4_addr, pkt) {
            warn!("could not emit packet: {:?}", e);
            return;
        }

        // Post-send operations follow.
        // Review: We perform these after the send, in order to keep send latency as low as possible.

        // Since we sent an ACK, cancel any outstanding delayed ACK request.
        cb.delivery.ack_deadline_time_secs.set(None);
    }

    //======================================================================================================================
    // Receiver methods
    //======================================================================================================================

    // Block until some data is received, up to an optional size.
    pub async fn pop(
        &mut self,
        mut size: Option<usize>,
    ) -> Result<ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        let mut bufs = ArrayVec::new();
        let mut buf = self.pop_queue.pop(None).await?;
        loop {
            if let Some(size) = size.as_mut() {
                if buf.len() > *size {
                    let remaining_buf = buf.split_front(*size)?;
                    self.pop_queue.push_front(remaining_buf);
                }
                *size -= buf.len();
            }
            match buf.len() {
                len if len > 0 => {
                    self.reader_next_seq_no = self.reader_next_seq_no + SeqNumber::from(buf.len() as u32);
                },
                _ => {
                    debug!("found FIN");
                    self.reader_next_seq_no = self.reader_next_seq_no + 1.into();
                    bufs.push(buf);

                    break;
                },
            }
            bufs.push(buf);
            match size {
                Some(0) => break,
                _ => match self.pop_queue.try_pop() {
                    Some(next_buf) => buf = next_buf,
                    None => break,
                },
            }
        }

        Ok(bufs)
    }

    // Receive a single incoming packet from layer3.
    pub fn receive(
        control_block: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        tcp_hdr: TcpHeader,
        buf: DemiBuffer,
        now: Instant,
    ) {
        match Self::process_packet(control_block, layer3_endpoint, tcp_hdr, buf, now) {
            Ok(()) => (),
            Err(e) => debug!("Dropped packet: {:?}", e),
        }
    }

    /// This is the main function for processing an incoming packet during the Established state when the connection is
    /// active. Each step in this function return Ok if there is further processing to be done and EBADMSG if the
    /// packet should be dropped after the step.
    fn process_packet(
        control_block: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        mut header: TcpHeader,
        mut data: DemiBuffer,
        now: Instant,
    ) -> Result<(), Fail> {
        let mut seg_start = header.seq_num;
        let mut seg_end = seg_start;
        let mut seg_len = data.len() as u32;

        // Check if the segment is in the receive window and trim off everything else.
        Self::check_segment_in_window(
            control_block,
            layer3_endpoint,
            &mut header,
            &mut data,
            &mut seg_start,
            &mut seg_end,
            &mut seg_len,
        )?;
        Self::check_and_process_rst(control_block, &header)?;
        Self::check_syn(&header)?;
        Self::check_and_process_ack(control_block, &header, now)?;

        // TODO: Check the URG bit.  If we decide to support this, how should we do it?
        if header.urg {
            warn!("Got packet with URG bit set!");
        }

        // Store whether the packet has data here because processing it will consume the DemiBuffer.
        let has_data = !data.is_empty();
        if has_data {
            Self::process_data(control_block, layer3_endpoint, data, seg_start, seg_end, seg_len)?;
        }
        // Deal with FIN flag, saving the FIN for later if it is out of order.
        Self::check_and_process_fin(control_block, &header, seg_end, layer3_endpoint)?;

        // We should ACK this segment, preferably via piggybacking on a response.
        if control_block.delivery.ack_deadline_time_secs.get().is_none() {
            // Start the delayed ACK timer to ensure an ACK gets sent soon even if no piggyback opportunity occurs.
            let timeout = control_block.delivery.ack_delay_timeout_secs;
            // Getting the current time is extremely cheap as it is just a variable lookup.
            control_block.delivery.ack_deadline_time_secs.set(Some(now + timeout));
        } else if has_data {
            // We already owe our peer an ACK (the timer was already running), so cancel the timer and ACK now.
            control_block.delivery.ack_deadline_time_secs.set(None);
            trace!("process_packet(): sending ack before deadline because another packet arrived");
            Self::send_ack(control_block, layer3_endpoint);
        }

        Ok(())
    }

    // This function causes a EOF to be returned to the user. We also know that there will be no more incoming
    // data after this sequence number.
    fn check_and_process_fin(
        cb: &mut ControlBlock,
        header: &TcpHeader,
        seg_end: SeqNumber,
        layer3_endpoint: &mut SharedLayer3Endpoint,
    ) -> Result<(), Fail> {
        if header.fin {
            match cb.delivery.recv_fin_seq_no.get() {
                // We've already received this FIN.
                Some(seq_no) if seg_end != seq_no => {
                    warn!(
                        "Received a FIN with a different sequence number, ignoring. previous={:?} new={:?}",
                        seq_no, seg_end,
                    )
                },
                Some(_) => (),
                None => {
                    trace!("Received FIN");
                    cb.delivery.recv_fin_seq_no.set(seg_end.into());
                },
            }
        };

        // Have we received all data before the FIN?
        if cb
            .delivery
            .recv_fin_seq_no
            .get()
            .is_some_and(|seq_no| seq_no == cb.delivery.receive_next_seq_no)
        {
            let state = match cb.connection_management.state {
                State::Established => State::CloseWait,
                State::FinWait1 => State::Closing,
                State::FinWait2 => State::TimeWait,
                state => unreachable!("Cannot be in any other state at this point: {:?}", state),
            };
            cb.connection_management.state = state;
            cb.delivery.pop_queue.push(DemiBuffer::new(0));
            debug_assert_eq!(
                cb.delivery.receive_next_seq_no,
                cb.delivery.recv_fin_seq_no.get().unwrap()
            );
            // Reset it to wake up any close coroutines waiting for FIN to arrive.
            cb.delivery.recv_fin_seq_no.set(Some(cb.delivery.receive_next_seq_no));
            // Move RECV_NXT over the FIN.
            cb.delivery.receive_next_seq_no = cb.delivery.receive_next_seq_no + 1.into();
        }

        // Have we processed all of the data and the FIN?
        if header.fin {
            Self::send_ack(cb, layer3_endpoint);
        }

        Ok(())
    }

    pub fn receive_window_size(&self) -> u32 {
        let bytes_unread: u32 = (self.receive_next_seq_no - self.reader_next_seq_no).into();
        // The window should be less than 1GB or 64KB without scaling.
        debug_assert!(
            (self.window_scale_shift_bits == 0 && bytes_unread <= MAX_WINDOW_SIZE_WITHOUT_SCALING)
                || bytes_unread <= MAX_WINDOW_SIZE_WITH_SCALING
        );
        debug!(
            "Receive window size: bytes_unread={:?} buffer_size_bytes={:?} ",
            bytes_unread, self.buffer_size_bytes
        );
        self.buffer_size_bytes - bytes_unread
    }

    pub fn hdr_window_size(&self) -> u16 {
        let window_size = self.receive_window_size();
        let hdr_window_size = expect_ok!(
            (window_size >> self.window_scale_shift_bits).try_into(),
            "Window size overflow"
        );
        debug!(
            "Window size -> {} (hdr {}, scale {})",
            (hdr_window_size as u32) << self.window_scale_shift_bits,
            hdr_window_size,
            self.window_scale_shift_bits,
        );
        hdr_window_size
    }

    // This routine takes an incoming in-order TCP segment and adds the data to the user's receive queue.  If the new
    // segment fills a "hole" in the receive sequence number space allowing previously stored out-of-order data to now
    // be received, it receives that too.
    //
    // This routine also updates receive_next to reflect any data now considered "received".
    fn receive_data(&mut self, seg_start: SeqNumber, buf: DemiBuffer) {
        // This routine should only be called with in-order segment data.
        debug_assert_eq!(seg_start, self.receive_next_seq_no);

        // Push the new segment data onto the end of the receive queue.
        self.receive_next_seq_no = self.receive_next_seq_no + SeqNumber::from(buf.len() as u32);
        // This inserts the segment and wakes a waiting pop coroutine.
        self.pop_queue.push(buf);

        // Okay, we've successfully received some new data.  Check if any of the formerly out-of-order data waiting in
        // the out-of-order queue is now in-order.  The out-of-order queue is ordered, so we can stop once the next
        // segment is not in sequence.
        while self
            .out_of_order_frames
            .front()
            .is_some_and(|(next_seq_no, _)| *next_seq_no == self.receive_next_seq_no)
        {
            // Move this entry's buffer from the out-of-order store to the receive queue.
            // This data is now considered to be "received" by TCP, and included in our RCV.NXT calculation.
            debug!("Recovering out-of-order packet at {}", self.receive_next_seq_no);
            let (_, buf) = self.out_of_order_frames.pop_front().unwrap();
            self.receive_next_seq_no = self.receive_next_seq_no + SeqNumber::from(buf.len() as u32);
            // This inserts the segment and wakes a waiting pop coroutine.
            self.pop_queue.push(buf);
        }
    }

    // Block until the remote sends a FIN (plus all previous data has arrived).
    pub async fn wait_for_fin(&mut self) -> Result<(), Fail> {
        let mut fin_seq_no = self.recv_fin_seq_no.get();
        loop {
            match fin_seq_no {
                Some(fin_seq_no) if self.receive_next_seq_no >= fin_seq_no => return Ok(()),
                _ => {
                    fin_seq_no = self.recv_fin_seq_no.wait_for_change(None).await?;
                },
            }
        }
    }

    // Check to see if the segment is acceptable sequence-wise (i.e. contains some data that fits within the receive
    // window, or is a non-data segment with a sequence number that falls within the window).  Unacceptable segments
    // should be ACK'd (unless they are RSTs), and then dropped.
    // Returns Ok if further processing is needed and EBADMSG if the packet is not within the receive window.
    fn check_segment_in_window(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
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

        let receive_next = cb.delivery.receive_next_seq_no;

        let after_receive_window = receive_next + SeqNumber::from(cb.delivery.receive_window_size());

        // Check if this segment fits in our receive window.
        // In the optimal case it starts at RCV.NXT, so we check for that first.
        if *seg_start != receive_next {
            // The start of this segment is not what we expected.  See if it comes before or after.
            if *seg_start < receive_next {
                // This segment contains duplicate data (i.e. data we've already received).
                // See if it is a complete duplicate, or if some of the data is new.
                if *seg_end < receive_next {
                    // This is an entirely duplicate (i.e. old) segment.  ACK (if not RST) and drop.
                    if !header.rst {
                        trace!("check_segment_in_window(): send ack on duplicate segment");
                        Self::send_ack(cb, layer3_endpoint);
                    }
                    let cause = "duplicate packet";
                    error!("check_segment_in_window(): {}", cause);
                    return Err(Fail::new(libc::EBADMSG, cause));
                } else {
                    // Some of this segment's data is new.  Cut the duplicate data off of the front.
                    // If there is a SYN at the start of this segment, remove it too.
                    let mut duplicate = u32::from(receive_next - *seg_start);
                    *seg_start = *seg_start + SeqNumber::from(duplicate);
                    *seg_len -= duplicate;
                    if header.syn {
                        header.syn = false;
                        duplicate -= 1;
                    }
                    expect_ok!(
                        data.adjust(duplicate as usize),
                        "'data' should contain at least 'duplicate' bytes"
                    );
                }
            } else {
                // This segment contains entirely new data, but is later in the sequence than what we're expecting.
                // See if any part of the data fits within our receive window.
                if *seg_start >= after_receive_window {
                    // This segment is completely outside of our window.  ACK (if not RST) and drop.
                    if !header.rst {
                        trace!("check_segment_in_window(): send ack on out-of-window segment");
                        Self::send_ack(cb, layer3_endpoint);
                    }
                    let cause = "packet outside of receive window";
                    error!("check_segment_in_window(): {}", cause);
                    return Err(Fail::new(libc::EBADMSG, cause));
                }

                // At least the beginning of this segment is in the window.  We'll check the end below.
            }
        }

        // The start of the segment is in the window.
        // Check that the end of the segment is in the window, and trim it down if it is not.
        if *seg_len > 0 && *seg_end >= after_receive_window {
            let mut excess = u32::from(*seg_end - after_receive_window);
            excess += 1;
            // TODO: If we end up (after receive handling rewrite is complete) not needing seg_end and seg_len after
            // this, remove these two lines adjusting them as they're being computed needlessly.
            *seg_end = *seg_end - SeqNumber::from(excess);
            *seg_len -= excess;
            if header.fin {
                header.fin = false;
                excess -= 1;
            }
            expect_ok!(
                data.trim(excess as usize),
                "'data' should contain at least 'excess' bytes"
            );
        }

        // From here on, the entire new segment (including any SYN or FIN flag remaining) is in the window.
        // Note that one interpretation of RFC 793 would have us store away (or just drop) any out-of-order packets at
        // this point, and only proceed onwards if seg_start == receive_next.  But we process any RSTs, SYNs, or ACKs
        // we receive (as long as they're in the window) as we receive them, even if they're out-of-order.  It's only
        // when we get to processing the data (and FIN) that we store aside any out-of-order segments for later.
        debug_assert!(receive_next <= *seg_start && *seg_end < after_receive_window);
        Ok(())
    }

    // TODO: RFC 5961 "Blind Reset Attack Using the RST Bit" prevention would have us ACK and drop if the new segment
    // doesn't start precisely on RCV.NXT.
    fn check_and_process_rst(cb: &mut ControlBlock, header: &TcpHeader) -> Result<(), Fail> {
        if !header.rst {
            return Ok(());
        }
        info!("Received RST: remote reset connection");
        match cb.delivery.recv_fin_seq_no.get() {
            // We've already received a FIN.
            Some(seq_no) if seq_no > header.seq_num => {
                warn!(
                    "Received a RST with a lower sequence number, updating. previous={:?} new={:?}",
                    seq_no, header.seq_num,
                )
            },
            Some(_) => (),
            None => {
                trace!("Received FIN");
                cb.delivery.recv_fin_seq_no.set(Some(header.seq_num));
            },
        }
        cb.connection_management.state = State::Closed;
        Err(Fail::new(libc::ECONNRESET, "remote reset connection"))
    }

    // Check the SYN bit.
    fn check_syn(header: &TcpHeader) -> Result<(), Fail> {
        // Note: RFC 793 says to check security/compartment and precedence next, but those are largely deprecated.

        // Check the SYN bit.
        if header.syn {
            // TODO: RFC 5961 "Blind Reset Attack Using the SYN Bit" prevention would have us always ACK and drop here.

            // Receiving a SYN here is an error.
            let cause = "Received in-window SYN on established connection.";
            error!("{}", cause);
            // TODO: Send Reset.
            // TODO: Return all outstanding Receive and Send requests with "reset" responses.
            // TODO: Flush all segment queues.

            // TODO: Start the close coroutine
            return Err(Fail::new(libc::EBADMSG, cause));
        }
        Ok(())
    }

    // Check the ACK bit.
    fn check_and_process_ack(cb: &mut ControlBlock, header: &TcpHeader, now: Instant) -> Result<(), Fail> {
        if !header.ack {
            // All segments on established connections should be ACKs.  Drop this segment.
            let cause = "Received non-ACK segment on established connection";
            error!("{}", cause);
            return Err(Fail::new(libc::EBADMSG, cause));
        }

        // TODO: RFC 5961 "Blind Data Injection Attack" prevention would have us perform additional ACK validation
        // checks here.

        Self::process_ack(cb, header, now);

        Ok(())
    }

    fn process_data(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        data: DemiBuffer,
        seg_start: SeqNumber,
        seg_end: SeqNumber,
        seg_len: u32,
    ) -> Result<(), Fail> {
        // TCP dictates that we only receive data in these states.
        match cb.connection_management.state {
            State::Established | State::FinWait1 | State::FinWait2 => (),
            state => {
                warn!("Ignoring data received after FIN (in state {:?}).", state);
                return Ok(());
            },
        };

        // Data is in order, so directly receive.
        if seg_start == cb.delivery.receive_next_seq_no {
            cb.delivery.receive_data(seg_start, data);
            return Ok(());
        }

        // This segment is out-of-order.  If it carries data, we should store it for later processing
        // after the "hole" in the sequence number space has been filled.
        debug!(
            "Received out-of-order segment; out_of_order_frames.len() = {:?}",
            cb.delivery.out_of_order_frames.len()
        );
        debug_assert_ne!(seg_len, 0);
        debug_assert_eq!(seg_len, data.len() as u32);
        cb.delivery.store_out_of_order_segment(seg_start, seg_end, data);
        // Sending an ACK here is only a "MAY" according to the RFCs, but helpful for fast retransmit.
        trace!("process_data(): send ack on out-of-order segment");
        Self::send_ack(cb, layer3_endpoint);

        // We're done with this out-of-order segment.
        Ok(())
    }

    // This routine takes an incoming TCP segment and adds it to the out-of-order receive queue.
    // If the new segment had a FIN it has been removed prior to this routine being called.
    // Note: Since this is not the "fast path", this is written for clarity over efficiency.
    //
    fn store_out_of_order_segment(&mut self, mut new_start: SeqNumber, mut new_end: SeqNumber, mut buf: DemiBuffer) {
        let mut action_index = self.out_of_order_frames.len();
        let mut another_pass_neeeded = true;

        while another_pass_neeeded {
            another_pass_neeeded = false;

            // Find the new segment's place in the out-of-order store.
            // The out-of-order store is sorted by starting sequence number, and contains no duplicate data.
            action_index = self.out_of_order_frames.len();
            for index in 0..self.out_of_order_frames.len() {
                let stored_segment = &self.out_of_order_frames[index];

                // Properties of the segment stored at this index.
                let stored_start = stored_segment.0;
                let stored_len = stored_segment.1.len() as u32;
                debug_assert_ne!(stored_len, 0);
                let stored_end = stored_start + SeqNumber::from(stored_len - 1);

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
                    let excess = u32::from(new_end - stored_start) + 1;
                    new_end = new_end - SeqNumber::from(excess);
                    expect_ok!(
                        buf.trim(excess as usize),
                        "'buf' should contain at least 'excess' bytes"
                    );
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
                    let duplicate = u32::from(stored_end - new_start);
                    new_start = new_start + SeqNumber::from(duplicate);
                    expect_ok!(
                        buf.adjust(duplicate as usize),
                        "'buf' should contain at least 'duplicate' bytes"
                    );
                    continue;
                }
            }

            if another_pass_neeeded {
                // The new segment completely encompassed an existing segment, which we will now remove.
                self.out_of_order_frames.remove(action_index);
            }
        }

        // Insert the new segment into the correct position.
        self.out_of_order_frames.insert(action_index, (new_start, buf));

        // If the out-of-order store now contains too many entries, delete the later entries.
        // TODO: The out-of-order store is already limited (in size) by our receive window, while the below check
        // imposes a limit on the number of entries.  Do we need this?  Presumably for attack mitigation?
        while self.out_of_order_frames.len() > MAX_OUT_OF_ORDER_SIZE_FRAMES {
            self.out_of_order_frames.pop_back();
        }
    }

    pub async fn acknowledger(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
    ) -> Result<Never, Fail> {
        let mut ack_deadline = cb.delivery.ack_deadline_time_secs.clone();
        let mut deadline = ack_deadline.get();

        loop {
            // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
            // - TCP should implement a delayed ACK
            // - The delay must be less than 500ms
            // - For a stream of full-sized segments, there should be an ack for every other segment.
            // TODO: Implement SACKs
            match ack_deadline.wait_for_change_until(deadline).await {
                Ok(value) => {
                    deadline = value;
                    continue;
                },
                Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => {
                    Self::send_ack(cb, layer3_endpoint);
                    deadline = ack_deadline.get();
                },
                Err(_) => {
                    unreachable!(
                        "either the ack deadline changed or the deadline passed, no other errors are possible!"
                    )
                },
            }
        }
    }
}
