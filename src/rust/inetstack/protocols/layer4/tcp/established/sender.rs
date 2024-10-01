// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    collections::{async_queue::SharedAsyncQueue, async_value::SharedAsyncValue},
    expect_ok, expect_some,
    inetstack::protocols::layer4::tcp::{established::SharedControlBlock, header::TcpHeader, SeqNumber},
    runtime::{conditional_yield_until, fail::Fail, memory::DemiBuffer},
};
use ::futures::{pin_mut, select_biased, FutureExt};
use ::libc::{EBUSY, EINVAL};
use ::std::{
    fmt,
    time::{Duration, Instant},
};
use futures::never::Never;
use std::cmp;

// Structure of entries on our unacknowledged queue.
// TODO: We currently allocate these on the fly when we add a buffer to the queue.  Would be more efficient to have a
// buffer structure that held everything we need directly, thus avoiding this extra wrapper.
//
pub struct UnackedSegment {
    pub bytes: DemiBuffer,
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

// TODO: Consider moving retransmit timer and congestion control fields out of this structure.
// TODO: Make all public fields in this structure private.
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

    // Sequence Number of the next data to be sent.  In RFC 793 terms, this is SND.NXT.
    send_next: SharedAsyncValue<SeqNumber>,

    // This is the send buffer (user data we do not yet have window to send).
    unsent_queue: SharedAsyncQueue<DemiBuffer>,

    // Available window to send into, as advertised by our peer.  In RFC 793 terms, this is SND.WND.
    send_window: SharedAsyncValue<u32>,
    send_window_last_update_seq: SeqNumber, // SND.WL1
    send_window_last_update_ack: SeqNumber, // SND.WL2

    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    send_window_scale_shift_bits: u8,

    // Maximum Segment Size currently in use for this connection.
    // TODO: Revisit this once we support path MTU discovery.
    mss: usize,
}

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("send_unacked", &self.send_unacked)
            .field("send_next", &self.send_next)
            .field("send_window", &self.send_window)
            .field("window_scale", &self.send_window_scale_shift_bits)
            .field("mss", &self.mss)
            .finish()
    }
}

impl Sender {
    pub fn new(seq_no: SeqNumber, send_window: u32, send_window_scale_shift_bits: u8, mss: usize) -> Self {
        Self {
            send_unacked: SharedAsyncValue::new(seq_no),
            unacked_queue: SharedAsyncQueue::with_capacity(MIN_UNACKED_QUEUE_SIZE_FRAMES),
            send_next: SharedAsyncValue::new(seq_no),
            unsent_queue: SharedAsyncQueue::with_capacity(MIN_UNSENT_QUEUE_SIZE_FRAMES),

            send_window: SharedAsyncValue::new(send_window),
            send_window_last_update_seq: seq_no,
            send_window_last_update_ack: seq_no,

            send_window_scale_shift_bits,
            mss,
        }
    }

    // This is the main TCP send routine.
    //
    pub fn immediate_send(&mut self, buf: DemiBuffer, mut cb: SharedControlBlock) -> Result<(), Fail> {
        // If the user is done sending (i.e. has called close on this connection), then they shouldn't be sending.

        // Our API supports send buffers up to usize (variable, depends upon architecture) in size.  While we could
        // allow for larger send buffers, it is simpler and more practical to limit a single send to 1 GiB, which is
        // also the maximum value a TCP can advertise as its receive window (with maximum window scaling).
        // TODO: the below check just limits a single send to 4 GiB, not 1 GiB.  Check this doesn't break anything.
        //
        // Review: Move this check up the stack (i.e. closer to the user)?
        //
        let mut buf_len: u32 = buf
            .len()
            .try_into()
            .map_err(|_| Fail::new(EINVAL, "buffer too large"))?;

        // TODO: What we should do here:
        //
        // Conceptually, we should take the provided buffer and add it to the unsent queue.  Then calculate the amount
        // of data we're currently allowed to send (based off of the receiver's advertised window, our congestion
        // control algorithm, silly window syndrome avoidance algorithm, etc).  Finally, enter a loop where we compose
        // maximum sized segments from the data on the unsent queue and send them, saving a (conceptual) copy of the
        // sent data on the unacknowledged queue, until we run out of either window space or unsent data.
        //
        // Note that there are several shortcuts we can make to this conceptual approach to speed the common case.
        // Note also that this conceptual send code is almost identical to what our "background send" algorithm should
        // be doing, so we should just have a single function that we call from both places.
        //
        // The current code below just tries to send the provided buffer immediately (if allowed), otherwise it places
        // it on the unsent queue and that's it.
        //

        // Check for unsent data.
        if self.unsent_queue.is_empty() {
            // No unsent data queued up, so we can try to send this new buffer immediately.

            // Calculate amount of data in flight (SND.NXT - SND.UNA).
            let send_unacknowledged: SeqNumber = self.send_unacked.get();
            let send_next: SeqNumber = self.send_next.get();
            let sent_data: u32 = (send_next - send_unacknowledged).into();

            // TODO: What limits buffer len to MSS?
            let in_flight_after_send: u32 = sent_data + buf_len;

            // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
            cb.congestion_control_on_cwnd_check_before_send();
            let cwnd: SharedAsyncValue<u32> = cb.congestion_control_get_cwnd();

            // The limited transmit algorithm can increase the effective size of cwnd by up to 2MSS.
            let effective_cwnd: u32 = cwnd.get() + cb.congestion_control_get_limited_transmit_cwnd_increase().get();

            let win_sz: u32 = self.send_window.get();

            if win_sz > 0 && win_sz >= in_flight_after_send && effective_cwnd >= in_flight_after_send {
                // This hook is primarily intended to record the last time we sent data, so we can later tell if
                // the connection has been idle.
                let rto: Duration = cb.rto();
                cb.congestion_control_on_send(rto, sent_data);

                // Prepare the segment and send it.
                let mut header: TcpHeader = cb.tcp_header();
                header.seq_num = send_next;
                if buf_len == 0 {
                    // This buffer is the end-of-send marker.
                    // Set FIN and adjust sequence number consumption accordingly.
                    header.fin = true;
                    buf_len = 1;
                } else {
                    header.psh = true;
                }
                trace!("Send immediate");
                cb.emit(header, Some(buf.clone()));

                // Update SND.NXT.
                self.send_next.modify(|s| s + SeqNumber::from(buf_len));

                // Put the segment we just sent on the retransmission queue.
                let unacked_segment = UnackedSegment {
                    bytes: buf,
                    initial_tx: Some(cb.get_now()),
                };
                self.unacked_queue.push(unacked_segment);

                // Start the retransmission timer if it isn't already running.
                if cb.get_retransmit_deadline().is_none() {
                    let rto: Duration = cb.rto();
                    cb.set_retransmit_deadline(Some(cb.get_now() + rto));
                }

                return Ok(());
            }
        }

        // Too fast.
        // TODO: We need to fix this the correct way: limit our send buffer size to the amount we're willing to buffer.
        if self.unsent_queue.len() > UNSENT_QUEUE_CUTOFF {
            return Err(Fail::new(EBUSY, "too many packets to send"));
        }

        // Slow path: Delegating sending the data to background processing.
        trace!("Queueing Send for background processing");
        self.unsent_queue.push(buf);

        Ok(())
    }

    pub async fn background_sender(&mut self, mut cb: SharedControlBlock) -> Result<Never, Fail> {
        'top: loop {
            // First, check to see if there's any unsent data.
            let mut unsent_segment: DemiBuffer = self.unsent_queue.pop(None).await?;

            // Okay, we know we have some unsent data past this point. Next, check to see that the
            // remote side has available window.
            let mut win_sz_watched: SharedAsyncValue<u32> = self.send_window.clone();
            let win_sz: u32 = win_sz_watched.get();

            // If we don't have any window size at all, we need to transition to PERSIST mode and
            // repeatedly send window probes until window opens up.
            if win_sz == 0 {
                // Send a window probe (this is a one-byte packet designed to elicit a window update from our peer).
                let buf: DemiBuffer = unsent_segment.split_front(1)?;
                // Put unsent data back in the queue.
                self.unsent_queue.push_front(unsent_segment);
                // Update SND.NXT.
                self.send_next.modify(|s| s + SeqNumber::from(1));

                // Add the probe byte (as a new separate buffer) to our unacknowledged queue.
                let unacked_segment = UnackedSegment {
                    bytes: buf.clone(),
                    initial_tx: Some(cb.get_now()),
                };
                self.unacked_queue.push(unacked_segment);

                // Note that we loop here *forever*, exponentially backing off.
                // TODO: Use the correct PERSIST mode timer here.
                let mut timeout: Duration = Duration::from_secs(1);
                loop {
                    // Create packet.
                    let mut header: TcpHeader = cb.tcp_header();
                    header.seq_num = self.send_next.get();
                    cb.emit(header, Some(buf.clone()));

                    match win_sz_watched.wait_for_change(Some(timeout)).await {
                        Ok(_) => continue 'top,
                        Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => timeout *= 2,
                        Err(_) => unreachable!(
                            "either the ack deadline changed or the deadline passed, no other errors are possible!"
                        ),
                    }
                }
            }

            // The remote window is nonzero, but there still may not be room.
            let mut send_unacked_watched: SharedAsyncValue<SeqNumber> = self.send_unacked.clone();
            let send_unacked: SeqNumber = send_unacked_watched.get();

            // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
            cb.congestion_control_on_cwnd_check_before_send();
            let mut cwnd_watched: SharedAsyncValue<u32> = cb.congestion_control_get_cwnd();
            let cwnd: u32 = cwnd_watched.get();

            // The limited transmit algorithm may increase the effective size of cwnd by up to 2 * mss.
            let mut ltci_watched: SharedAsyncValue<u32> = cb.congestion_control_get_limited_transmit_cwnd_increase();
            let ltci: u32 = ltci_watched.get();

            let effective_cwnd: u32 = cwnd + ltci;
            let next_buf_size: usize = expect_some!(self.unsent_queue.get_front(), "no buffer in unsent queue").len();

            let sent_data: u32 = (self.send_next.get() - send_unacked).into();
            if win_sz <= (sent_data + next_buf_size as u32)
                || effective_cwnd <= sent_data
                || (effective_cwnd - sent_data) <= self.mss as u32
            {
                futures::select_biased! {
                    _ = send_unacked_watched.wait_for_change(None).fuse() => continue 'top,
                    _ = self.send_next.wait_for_change(None).fuse() => continue 'top,
                    _ = win_sz_watched.wait_for_change(None).fuse() => continue 'top,
                    _ = cwnd_watched.wait_for_change(None).fuse() => continue 'top,
                    _ = ltci_watched.wait_for_change(None).fuse() => continue 'top,
                };
            }

            // Past this point we have data to send and it's valid to send it!

            // TODO: Nagle's algorithm - We need to coalese small buffers together to send MSS sized packets.
            // TODO: Silly window syndrome - See RFC 1122's discussion of the SWS avoidance algorithm.

            // Form an outgoing packet.
            let max_frame_size_bytes: usize = cmp::min(
                cmp::min((win_sz - sent_data) as usize, self.mss),
                (effective_cwnd - sent_data) as usize,
            );

            // Split the packet if necessary.
            // TODO: Use a scatter/gather array to coalesce multiple buffers into a single segment.
            let (segment_data, do_push): (DemiBuffer, bool) = {
                let buf_len: usize = unsent_segment.len();

                if buf_len > max_frame_size_bytes {
                    let mut cloned_buf: DemiBuffer = unsent_segment.clone();

                    expect_ok!(
                        unsent_segment.adjust(max_frame_size_bytes),
                        "'buf' should contain at least 'max_bytes'"
                    );
                    expect_ok!(
                        cloned_buf.trim(buf_len - max_frame_size_bytes),
                        "'cloned_buf' should contain at least less than its length"
                    );

                    self.unsent_queue.push_front(unsent_segment);

                    // Suppress PSH flag for partial buffers.
                    (cloned_buf, false)
                } else {
                    // We can just send the whole packet.
                    (unsent_segment, true)
                }
            };
            let mut segment_data_len: u32 = segment_data.len() as u32;

            let rto: Duration = cb.rto();
            cb.congestion_control_on_send(rto, sent_data);

            // Prepare the segment and send it.
            let mut header: TcpHeader = cb.tcp_header();
            header.seq_num = self.send_next.get();
            if segment_data_len == 0 {
                // This buffer is the end-of-send marker.
                // Set FIN and adjust sequence number consumption accordingly.
                header.fin = true;
                segment_data_len = 1;
            } else if do_push {
                header.psh = true;
            }
            let mut cb4 = cb.clone();
            cb4.emit(header, Some(segment_data.clone()));

            // Update SND.NXT.
            self.send_next.modify(|s| s + SeqNumber::from(segment_data_len));

            // Put this segment on the unacknowledged list.
            let unacked_segment = UnackedSegment {
                bytes: segment_data,
                initial_tx: Some(cb.get_now()),
            };
            self.unacked_queue.push(unacked_segment);

            // Set the retransmit timer.
            // TODO: Fix how the retransmit timer works.
            let retransmit_deadline = cb.get_retransmit_deadline();
            if retransmit_deadline.is_none() {
                let rto: Duration = cb.rto();
                cb.set_retransmit_deadline(Some(cb.get_now() + rto));
            }
        }
    }

    pub async fn background_retransmitter(&mut self, mut cb: SharedControlBlock) -> Result<Never, Fail> {
        // Watch the retransmission deadline.
        let mut rtx_deadline_watched: SharedAsyncValue<Option<Instant>> = cb.watch_retransmit_deadline();
        // Watch the fast retransmit flag.
        let mut rtx_fast_retransmit_watched: SharedAsyncValue<bool> = cb.congestion_control_watch_retransmit_now_flag();
        loop {
            let rtx_deadline: Option<Instant> = rtx_deadline_watched.get();
            let rtx_fast_retransmit: bool = rtx_fast_retransmit_watched.get();
            if rtx_fast_retransmit {
                // Notify congestion control about fast retransmit.
                cb.congestion_control_on_fast_retransmit();

                // Retransmit earliest unacknowledged segment.
                self.retransmit(&mut cb);
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
                Ok(()) => continue,
                Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => {
                    // Retransmit timeout.

                    // Notify congestion control about RTO.
                    // TODO: Is this the best place for this?
                    // TODO: Why call into ControlBlock to get SND.UNA when congestion_control_on_rto() has access to it?
                    cb.congestion_control_on_rto(self.send_unacked.get());

                    // RFC 6298 Section 5.4: Retransmit earliest unacknowledged segment.
                    self.retransmit(&mut cb);

                    // RFC 6298 Section 5.5: Back off the retransmission timer.
                    cb.clone().rto_back_off();

                    // RFC 6298 Section 5.6: Restart the retransmission timer with the new RTO.
                    let rto: Duration = cb.rto();
                    let deadline: Instant = cb.get_now() + rto;
                    cb.set_retransmit_deadline(Some(deadline));
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
    pub fn retransmit(&mut self, cb: &mut SharedControlBlock) {
        if self.unacked_queue.is_empty() {
            return;
        }
        let segment: &mut UnackedSegment = self
            .unacked_queue
            .get_front_mut()
            .expect("just checked if queue is empty");

        // We're retransmitting this, so we can no longer use an ACK for it as an RTT measurement (as we can't tell
        // if the ACK is for the original or the retransmission).  Remove the transmission timestamp from the entry.
        segment.initial_tx.take();

        // Clone the segment data for retransmission.
        let data: DemiBuffer = segment.bytes.clone();

        // TODO: Issue #198 Repacketization - we should send a full MSS (and set the FIN flag if applicable).

        // Prepare and send the segment.
        let mut header: TcpHeader = cb.tcp_header();
        header.seq_num = self.send_unacked.get();
        if data.len() == 0 {
            // This buffer is the end-of-send marker.  Retransmit the FIN.
            header.fin = true;
        } else {
            header.psh = true;
        }
        cb.emit(header, Some(data));
    }

    // This segment acknowledges new data, so process the ack.
    pub fn process_ack(&mut self, mut cb: SharedControlBlock, header: &TcpHeader, now: Instant) {
        let bytes_acknowledged: u32 = (header.ack_num - self.send_unacked.get()).into();
        let mut bytes_remaining: usize = bytes_acknowledged as usize;
        // Remove bytes from the unacked queue.
        while bytes_remaining != 0 {
            if let Some(mut segment) = self.unacked_queue.try_pop() {
                // Add sample for RTO if we have an initial transmit time.
                // Note that in the case of repacketization, an ack for the first byte is enough for the time sample.
                // TODO: TCP timestamp support.
                if let Some(initial_tx) = segment.initial_tx {
                    cb.rto_add_sample(now - initial_tx);
                }

                if segment.bytes.len() > bytes_remaining {
                    // Only some of the data in this segment has been acked.  Remove just the acked amount.
                    expect_ok!(
                        segment.bytes.adjust(bytes_remaining),
                        "'segment' should contain at least 'bytes_remaining'"
                    );
                    segment.initial_tx = None;

                    // Leave this segment on the unacknowledged queue.
                    self.unacked_queue.push_front(segment);
                    break;
                }

                if segment.bytes.len() == 0 {
                    // This buffer is the end-of-send marker.  So we should only have one byte of acknowledged sequence
                    // space remaining (corresponding to our FIN).
                    debug_assert_eq!(bytes_remaining, 1);
                    bytes_remaining = 0;
                }

                bytes_remaining -= segment.bytes.len();
            } else {
                debug_assert!(false); // Shouldn't have bytes_remaining with no segments remaining in unacked_queue.
            }
        }

        // Update SND.UNA to SEG.ACK.
        self.send_unacked.set(header.ack_num);

        // Update send window.
        // Check that the ACK we're using to update the window isn't older than the last one used to update it.
        if self.send_window_last_update_seq < header.seq_num
            || (self.send_window_last_update_seq == header.seq_num
                && self.send_window_last_update_ack <= header.ack_num)
        {
            // Update our send window.
            self.send_window
                .set((header.window_size as u32) << self.send_window_scale_shift_bits);
            self.send_window_last_update_seq = header.seq_num;
            self.send_window_last_update_ack = header.ack_num;
        }

        debug!(
            "Updating window size -> {} (hdr {}, scale {})",
            self.send_window.get(),
            header.window_size,
            self.send_window_scale_shift_bits,
        );
    }

    // Get SND.UNA.
    pub fn get_unacked_seq_no(&self) -> SeqNumber {
        self.send_unacked.get()
    }

    // Get SND.NXT.
    pub fn get_next_seq_no(&self) -> SeqNumber {
        self.send_next.get()
    }
}
