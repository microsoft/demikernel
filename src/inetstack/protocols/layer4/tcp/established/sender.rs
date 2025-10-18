// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::{async_queue::SharedAsyncQueue, async_value::SharedAsyncValue},
    inetstack::{
        consts::{MAX_BATCH_SIZE_NUM_PACKETS, MAX_HEADER_SIZE},
        protocols::{
            layer3::SharedLayer3Endpoint,
            layer4::tcp::{
                established::{ctrlblk::State, ControlBlock},
                header::TcpHeader,
                SeqNumber,
            },
        },
    },
    runtime::{conditional_yield_until, fail::Fail, memory::DemiBuffer, SharedDemiRuntime},
};
use ::arrayvec::ArrayVec;
use ::futures::{never::Never, pin_mut, select_biased, FutureExt};
use ::std::{
    cmp, fmt,
    time::{Duration, Instant},
};

//======================================================================================================================
// Data Structures
//======================================================================================================================

// Structure of entries on our unacknowledged queue.
// TODO: We currently allocate these on the fly when we add a buffer to the queue.  Would be more efficient to have a
// buffer structure that held everything we need directly, thus avoiding this extra wrapper.
//
pub struct UnackedSegment {
    pub bytes: Option<DemiBuffer>,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

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

pub struct Sender {
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
    fin_seq_no: Option<SeqNumber>,

    // This is the send buffer (user data we do not yet have window to send). If the option is None, then it indicates
    // a FIN. This keeps us from having to allocate an empty Demibuffer to indicate FIN.
    unsent_queue: SharedAsyncQueue<DemiBuffer>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Sender {
    pub fn new(local_seq_no: SeqNumber) -> Self {
        Self {
            send_unacked: SharedAsyncValue::new(local_seq_no),
            unacked_queue: SharedAsyncQueue::with_capacity(MIN_UNACKED_QUEUE_SIZE_FRAMES),
            retransmit_deadline_time_secs: SharedAsyncValue::new(None),
            send_next_seq_no: SharedAsyncValue::new(local_seq_no),
            unsent_next_seq_no: local_seq_no,
            fin_seq_no: None,
            unsent_queue: SharedAsyncQueue::with_capacity(MIN_UNSENT_QUEUE_SIZE_FRAMES),
        }
    }

    fn process_acked_fin(cb: &mut ControlBlock, bytes_remaining: usize, ack_num: SeqNumber) -> usize {
        // This buffer is the end-of-send marker.  So we should only have one byte of acknowledged
        // sequence space remaining (corresponding to our FIN).
        debug_assert_eq!(bytes_remaining, 1);

        // Double check that the ack is for the FIN sequence number.
        debug_assert_eq!(
            ack_num,
            cb.delivery
                .sender
                .fin_seq_no
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
        debug_assert!(cb.delivery.sender.fin_seq_no.is_none());

        // TODO: We need to fix this the correct way: limit our send buffer size to the amount we're willing to buffer.
        if cb.delivery.sender.unsent_queue.len() > UNSENT_QUEUE_CUTOFF - 1 {
            return Err(Fail::new(libc::EBUSY, "too many packets to send"));
        }

        trace!(
            "push(): total unsent segments={:?}",
            cb.delivery.sender.unsent_queue.len()
        );

        // Check if closing the socket and sending FIN.
        if bufs.is_empty() {
            // We can always send the FIN immediately.
            cb.delivery.sender.fin_seq_no = Some(cb.delivery.sender.unsent_next_seq_no);
            cb.delivery.sender.unsent_next_seq_no = cb.delivery.sender.unsent_next_seq_no + 1.into();
            Self::send_fin(cb, layer3_endpoint, runtime.now())?;
        } else {
            for mut buf in bufs.into_iter() {
                cb.delivery.sender.unsent_next_seq_no =
                    cb.delivery.sender.unsent_next_seq_no + (buf.len() as u32).into();
                if cb.flow_control.send_window.get() > 0 {
                    Self::send_segment(cb, layer3_endpoint, runtime.now(), &mut buf);

                    if !buf.is_empty() {
                        cb.delivery.sender.unsent_queue.push(buf);
                    }
                }
            }
        }

        if !cb.delivery.sender.unacked_queue.is_empty() {
            trace!(
                "push(): total unacked segments={:?}",
                cb.delivery.sender.unacked_queue.len()
            );
        }

        // Wait until the sequnce number of the pushed buffer is acknowledged.
        let mut send_unacked_watched = cb.delivery.sender.send_unacked.clone();
        let ack_seq_no = cb.delivery.sender.unsent_next_seq_no;
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
            let buffer = cb.delivery.sender.unsent_queue.pop(None).await?;
            Self::send_buffer(cb, layer3_endpoint, runtime.now(), buffer).await?;
        }
    }

    fn send_fin(cb: &mut ControlBlock, layer3_endpoint: &mut SharedLayer3Endpoint, now: Instant) -> Result<(), Fail> {
        debug_assert!(cb.delivery.sender.fin_seq_no.is_some());

        let mut header = Self::tcp_header(cb, cb.delivery.sender.fin_seq_no);
        header.fin = true;
        Self::emit(cb, layer3_endpoint, header, None);
        // Update SND.NXT.
        cb.delivery.sender.send_next_seq_no.modify(|s| s + 1.into());

        // Add the FIN to our unacknowledged queue.
        let unacked_segment = UnackedSegment {
            bytes: None,
            initial_tx: Some(now),
        };
        cb.delivery.sender.unacked_queue.push(unacked_segment);
        // Set the retransmit timer.
        if cb.delivery.sender.retransmit_deadline_time_secs.get().is_none() {
            let rto = cb.congestion_control.rto_calculator.rto();
            cb.delivery.sender.retransmit_deadline_time_secs.set(Some(now + rto));
        }
        Ok(())
    }

    async fn send_buffer(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        now: Instant,
        mut buffer: DemiBuffer,
    ) -> Result<(), Fail> {
        let mut send_unacked_watched = cb.delivery.sender.send_unacked.clone();
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
                    _ = cb.delivery.sender.send_next_seq_no.wait_for_change(None).fuse() => (),
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
        cb.delivery.sender.send_next_seq_no.modify(|s| s + SeqNumber::from(1));

        // Add the probe byte (as a new separate buffer) to our unacknowledged queue.
        let unacked_segment = UnackedSegment {
            bytes: Some(probe.clone()),
            initial_tx: Some(now),
        };
        cb.delivery.sender.unacked_queue.push(unacked_segment);

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
            (cb.delivery.sender.send_next_seq_no.get() - cb.delivery.sender.send_unacked.get()).into(),
        );

        // Prepare the segment and send it.
        let mut header = Self::tcp_header(cb, None);
        if do_push {
            header.psh = true;
        }
        Self::emit(cb, layer3_endpoint, header, Some(segment_data.clone()));

        // Update SND.NXT.
        cb.delivery
            .sender
            .send_next_seq_no
            .modify(|s| s + SeqNumber::from(segment_data_len));

        // Put this segment on the unacknowledged list.
        let unacked_segment = UnackedSegment {
            bytes: Some(segment_data),
            initial_tx: Some(now),
        };

        if !cb.delivery.sender.unacked_queue.is_empty() {
            trace!(
                "send_segment(): unacked_queue.len() = {:?}",
                cb.delivery.sender.unacked_queue.len()
            );
        }

        cb.delivery.sender.unacked_queue.push(unacked_segment);

        // Set the retransmit timer.
        if cb.delivery.sender.retransmit_deadline_time_secs.get().is_none() {
            let rto = cb.congestion_control.rto_calculator.rto();
            cb.delivery.sender.retransmit_deadline_time_secs.set(Some(now + rto));
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
        header.window_size = cb.delivery.receiver.hdr_window_size();

        // Note that once we reach a synchronized state we always include a valid acknowledgement number.
        header.ack = true;
        header.ack_num = cb.delivery.receiver.receive_next_seq_no;
        header.seq_num = seq_num.unwrap_or(cb.delivery.sender.send_next_seq_no.get());

        header
    }

    fn get_open_window_size_bytes(cb: &mut ControlBlock) -> usize {
        // Calculate amount of data in flight (SND.NXT - SND.UNA).
        let send_unacknowledged = cb.delivery.sender.send_unacked.get();
        let send_next = cb.delivery.sender.send_next_seq_no.get();
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
        let mut rtx_deadline_watched = cb.delivery.sender.retransmit_deadline_time_secs.clone();
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
                Ok(()) => match cb.delivery.sender.fin_seq_no {
                    Some(fin_seq_no) if cb.delivery.sender.send_unacked.get() > fin_seq_no => {
                        return Err(Fail::new(libc::ECONNRESET, "connection closed"));
                    },
                    _ => continue,
                },
                Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => {
                    // Retransmit timeout.
                    // Notify congestion control about RTO.
                    cb.congestion_control
                        .cc_algorithm
                        .on_rto(cb.delivery.sender.send_unacked.get());

                    // RFC 6298 Section 5.4: Retransmit earliest unacknowledged segment.
                    Self::retransmit(cb, layer3_endpoint);

                    // RFC 6298 Section 5.5: Back off the retransmission timer.
                    cb.congestion_control.rto_calculator.back_off();

                    // RFC 6298 Section 5.6: Restart the retransmission timer with the new RTO.
                    let deadline = runtime.now() + cb.congestion_control.rto_calculator.rto();
                    cb.delivery.sender.retransmit_deadline_time_secs.set(Some(deadline));
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
        if let Some(segment) = cb.delivery.sender.unacked_queue.front_mut() {
            // We're retransmitting this, so we can no longer use an ACK for it as an RTT measurement (as we can't tell
            // if the ACK is for the original or the retransmission).  Remove the transmission timestamp from the entry.
            segment.initial_tx.take();

            // Clone the segment data here for retransmission.
            let data = segment.bytes.clone();

            // TODO: Issue #198 Repacketization - we should send a full MSS (and set the FIN flag if applicable).

            let mut header = Self::tcp_header(cb, Some(cb.delivery.sender.send_unacked.get()));

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
        let send_unacknowledged = cb.delivery.sender.send_unacked.get();
        // Check and update send window if necessary.
        cb.flow_control.update_send_window(header);

        if send_unacknowledged < header.ack_num {
            // Remove the now acknowledged data from the unacknowledged queue, update the acked sequence number
            // and update the sender window.

            // Convert the difference in sequence numbers into a u32.
            let bytes_acknowledged: u32 = (header.ack_num - cb.delivery.sender.send_unacked.get()).into();
            // Convert that into a usize for counting bytes to remove from the unacked queue.
            let mut bytes_remaining = bytes_acknowledged as usize;
            // Remove bytes from the unacked queue.
            while bytes_remaining != 0 {
                bytes_remaining = match cb.delivery.sender.unacked_queue.try_pop() {
                    Some(segment) if segment.bytes.is_none() => {
                        Self::process_acked_fin(cb, bytes_remaining, header.ack_num)
                    },
                    Some(segment) => {
                        // We add the sample outside the Sender function to separate state.
                        cb.congestion_control.add_sample(segment.initial_tx, now);
                        cb.delivery.sender.process_acked_segment(bytes_remaining, segment)
                    },
                    None => {
                        unreachable!("There should be enough data in the unacked_queue for the number of bytes acked")
                    }, // Shouldn't have bytes_remaining with no segments remaining in unacked_queue.
                };
            }

            // Update SND.UNA to SEG.ACK.
            cb.delivery.sender.send_unacked.set(header.ack_num);

            // Reset the retransmit timer if necessary. If there is more data that hasn't been acked, then set to the
            // next segment deadline, otherwise, do not set.
            let retransmit_deadline_time_secs = cb
                .delivery
                .sender
                .update_retransmit_deadline(now, cb.congestion_control.rto_calculator.rto());
            #[cfg(debug_assertions)]
            if retransmit_deadline_time_secs.is_none() {
                debug_assert_eq!(cb.delivery.sender.send_next_seq_no.get(), header.ack_num);
            }
            cb.delivery
                .sender
                .retransmit_deadline_time_secs
                .set(retransmit_deadline_time_secs);
        } else {
            // Duplicate ACK (doesn't acknowledge anything new).  We can mostly ignore this, except for fast-retransmit.
            // TODO: Implement fast-retransmit.  In which case, we'd increment our dup-ack counter here.
            trace!(
                "process_ack(): received duplicate ack ({:?}); unacked len = {:?}",
                header.ack_num,
                cb.delivery.sender.unacked_queue.len()
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
        cb.delivery.receiver.ack_deadline_time_secs.set(None);
    }
}
//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("send_unacked", &self.send_unacked)
            .field("send_next", &self.send_next_seq_no)
            .finish()
    }
}
