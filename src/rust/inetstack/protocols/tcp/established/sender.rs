// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::tcp::{
        established::SharedControlBlock,
        segment::TcpHeader,
        SeqNumber,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        watched::SharedWatchedValue,
    },
};
use ::libc::{
    EBUSY,
    EINVAL,
};
use ::std::{
    cell::{
        Cell,
        RefCell,
    },
    collections::VecDeque,
    convert::TryInto,
    fmt,
    time::{
        Duration,
        Instant,
    },
};

// Structure of entries on our unacknowledged queue.
// TODO: We currently allocate these on the fly when we add a buffer to the queue.  Would be more efficient to have a
// buffer structure that held everything we need directly, thus avoiding this extra wrapper.
//
pub struct UnackedSegment {
    pub bytes: DemiBuffer,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

/// Hard limit for unsent queue.
/// TODO: Remove this.  We should limit the unsent queue by either having a (configurable) send buffer size (in bytes,
/// not segments) and rejecting send requests that exceed that, or by limiting the user's send buffer allocations.
const UNSENT_QUEUE_CUTOFF: usize = 1024;

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
    pub send_unacked: SharedWatchedValue<SeqNumber>,

    // Queue of unacknowledged sent data.  RFC 793 calls this the "retransmission queue".
    unacked_queue: RefCell<VecDeque<UnackedSegment>>,

    // Sequence Number of the next data to be sent.  In RFC 793 terms, this is SND.NXT.
    send_next: SharedWatchedValue<SeqNumber>,

    // This is the send buffer (user data we do not yet have window to send).
    unsent_queue: RefCell<VecDeque<DemiBuffer>>,

    // TODO: Remove this as soon as sender.rs is fixed to not use it to tell if there is unsent data.
    unsent_seq_no: SharedWatchedValue<SeqNumber>,

    // Available window to send into, as advertised by our peer.  In RFC 793 terms, this is SND.WND.
    send_window: SharedWatchedValue<u32>,
    send_window_last_update_seq: Cell<SeqNumber>, // SND.WL1
    send_window_last_update_ack: Cell<SeqNumber>, // SND.WL2

    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    window_scale: u8,

    // Maximum Segment Size currently in use for this connection.
    // TODO: Revisit this once we support path MTU discovery.
    mss: usize,
}

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("send_unacked", &self.send_unacked)
            .field("send_next", &self.send_next)
            .field("unsent_seq_no", &self.unsent_seq_no)
            .field("send_window", &self.send_window)
            .field("window_scale", &self.window_scale)
            .field("mss", &self.mss)
            .finish()
    }
}

impl Sender {
    pub fn new(seq_no: SeqNumber, send_window: u32, window_scale: u8, mss: usize) -> Self {
        Self {
            send_unacked: SharedWatchedValue::new(seq_no),
            unacked_queue: RefCell::new(VecDeque::new()),
            send_next: SharedWatchedValue::new(seq_no),
            unsent_queue: RefCell::new(VecDeque::new()),
            unsent_seq_no: SharedWatchedValue::new(seq_no),

            send_window: SharedWatchedValue::new(send_window),
            send_window_last_update_seq: Cell::new(seq_no),
            send_window_last_update_ack: Cell::new(seq_no),

            window_scale,
            mss,
        }
    }

    pub fn get_mss(&self) -> usize {
        self.mss
    }

    pub fn get_send_window(&self) -> SharedWatchedValue<u32> {
        self.send_window.clone()
    }

    pub fn get_send_unacked(&self) -> SharedWatchedValue<SeqNumber> {
        self.send_unacked.clone()
    }

    pub fn get_send_next(&self) -> SharedWatchedValue<SeqNumber> {
        self.send_next.clone()
    }

    pub fn modify_send_next(&mut self, f: impl FnOnce(SeqNumber) -> SeqNumber) {
        self.send_next.modify(f)
    }

    pub fn get_unsent_seq_no(&self) -> SharedWatchedValue<SeqNumber> {
        self.unsent_seq_no.clone()
    }

    pub fn push_unacked_segment(&self, segment: UnackedSegment) {
        self.unacked_queue.borrow_mut().push_back(segment)
    }

    // This is the main TCP send routine.
    //
    pub fn send(&mut self, buf: DemiBuffer, mut cb: SharedControlBlock) -> Result<(), Fail> {
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
        if self.unsent_queue.borrow().is_empty() {
            // No unsent data queued up, so we can try to send this new buffer immediately.

            // Calculate amount of data in flight (SND.NXT - SND.UNA).
            let send_unacknowledged: SeqNumber = self.send_unacked.get();
            let send_next: SeqNumber = self.send_next.get();
            let sent_data: u32 = (send_next - send_unacknowledged).into();

            // TODO: What limits buffer len to MSS?
            let in_flight_after_send: u32 = sent_data + buf_len;

            // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
            cb.congestion_control_on_cwnd_check_before_send();
            let cwnd: SharedWatchedValue<u32> = cb.congestion_control_get_cwnd();

            // The limited transmit algorithm can increase the effective size of cwnd by up to 2MSS.
            let effective_cwnd: u32 = cwnd.get() + cb.congestion_control_get_limited_transmit_cwnd_increase().get();

            let win_sz: u32 = self.send_window.get();

            if win_sz > 0 && win_sz >= in_flight_after_send && effective_cwnd >= in_flight_after_send {
                if let Some(remote_link_addr) = cb.arp().try_query(cb.get_remote().ip().clone()) {
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
                    cb.emit(header, Some(buf.clone()), remote_link_addr);

                    // Update SND.NXT.
                    self.send_next.modify(|s| s + SeqNumber::from(buf_len));

                    // TODO: We don't need to track this.
                    self.unsent_seq_no.modify(|s| s + SeqNumber::from(buf_len));

                    // Put the segment we just sent on the retransmission queue.
                    let unacked_segment = UnackedSegment {
                        bytes: buf,
                        initial_tx: Some(cb.get_timer().now()),
                    };
                    self.unacked_queue.borrow_mut().push_back(unacked_segment);

                    // Start the retransmission timer if it isn't already running.
                    if cb.get_retransmit_deadline().is_none() {
                        let rto: Duration = cb.rto();
                        cb.set_retransmit_deadline(Some(cb.get_timer().now() + rto));
                    }

                    return Ok(());
                } else {
                    warn!("no ARP cache entry for send");
                }
            }
        }

        // Too fast.
        // TODO: We need to fix this the correct way: limit our send buffer size to the amount we're willing to buffer.
        if self.unsent_queue.borrow().len() > UNSENT_QUEUE_CUTOFF {
            return Err(Fail::new(EBUSY, "too many packets to send"));
        }

        // Slow path: Delegating sending the data to background processing.
        trace!("Queueing Send for background processing");
        self.unsent_queue.borrow_mut().push_back(buf);
        self.unsent_seq_no.modify(|s| s + SeqNumber::from(buf_len));

        Ok(())
    }

    /// Retransmits the earliest segment that has not (yet) been acknowledged by our peer.
    pub fn retransmit(&self, mut cb: SharedControlBlock) {
        // Check that we have an unacknowledged segment.
        if let Some(segment) = self.unacked_queue.borrow_mut().front_mut() {
            // We're retransmitting this, so we can no longer use an ACK for it as an RTT measurement (as we can't tell
            // if the ACK is for the original or the retransmission).  Remove the transmission timestamp from the entry.
            segment.initial_tx.take();

            // Clone the segment data for retransmission.
            let data: DemiBuffer = segment.bytes.clone();

            // TODO: Issue #198 Repacketization - we should send a full MSS (and set the FIN flag if applicable).

            // Prepare and send the segment.
            if let Some(first_hop_link_addr) = cb.arp().try_query(cb.get_remote().ip().clone()) {
                let mut header: TcpHeader = cb.tcp_header();
                header.seq_num = self.send_unacked.get();
                if data.len() == 0 {
                    // This buffer is the end-of-send marker.  Retransmit the FIN.
                    header.fin = true;
                } else {
                    header.psh = true;
                }
                cb.emit(header, Some(data), first_hop_link_addr);
            }
        } else {
            // We shouldn't enter the retransmit routine with an empty unacknowledged queue.  So maybe we should assert
            // here?  But this is relatively benign if it happens, and could be the result of a race-condition or a
            // mismanaged retransmission timer, so asserting would be over-reacting.
            warn!("Retransmission with empty unacknowledged queue?");
        }
    }

    // Remove acknowledged data from the unacknowledged (a.k.a. retransmission) queue.
    //
    pub fn remove_acknowledged_data(&self, mut cb: SharedControlBlock, bytes_acknowledged: u32, now: Instant) {
        let mut bytes_remaining: usize = bytes_acknowledged as usize;

        while bytes_remaining != 0 {
            if let Some(segment) = self.unacked_queue.borrow_mut().front_mut() {
                // Add sample for RTO if we have an initial transmit time.
                // Note that in the case of repacketization, an ack for the first byte is enough for the time sample.
                // TODO: TCP timestamp support.
                if let Some(initial_tx) = segment.initial_tx {
                    cb.rto_add_sample(now - initial_tx);
                }

                if segment.bytes.len() > bytes_remaining {
                    // Only some of the data in this segment has been acked.  Remove just the acked amount.
                    segment
                        .bytes
                        .adjust(bytes_remaining)
                        .expect("'segment' should contain at least 'bytes_remaining'");
                    segment.initial_tx = None;

                    // Leave this segment on the unacknowledged queue.
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

            // Remove this segment from the unacknowledged queue.
            // TODO: Mark the send operation associated with this buffer as complete, so the user can reuse the buffer.
            self.unacked_queue.borrow_mut().pop_front();
        }
    }

    pub fn pop_one_unsent_byte(&self) -> Option<DemiBuffer> {
        let mut queue = self.unsent_queue.borrow_mut();

        let buf = queue.front_mut()?;
        let mut cloned_buf = buf.clone();
        let buf_len: usize = buf.len();

        // Pop one byte off the buf still in the queue and all but one of the bytes on our clone.
        buf.adjust(1).expect("'buf' should contain at least one byte");
        cloned_buf
            .trim(buf_len - 1)
            .expect("'cloned_buf' should contain at least one less than its professed length");

        Some(cloned_buf)
    }

    pub fn pop_unsent(&self, max_bytes: usize) -> Option<(DemiBuffer, bool)> {
        // TODO: Use a scatter/gather array to coalesce multiple buffers into a single segment.
        let mut unsent_queue = self.unsent_queue.borrow_mut();
        let mut buf: DemiBuffer = unsent_queue.pop_front()?;
        let mut do_push: bool = true;
        let buf_len: usize = buf.len();

        if buf_len > max_bytes {
            let mut cloned_buf: DemiBuffer = buf.clone();

            buf.adjust(max_bytes)
                .expect("'buf' should contain at least 'max_bytes'");
            cloned_buf
                .trim(buf_len - max_bytes)
                .expect("'cloned_buf' should contain at least less than its length");

            unsent_queue.push_front(buf);
            buf = cloned_buf;

            // Suppress PSH flag for partial buffers.
            do_push = false;
        }
        Some((buf, do_push))
    }

    pub fn top_size_unsent(&self) -> Option<usize> {
        let unsent_queue = self.unsent_queue.borrow_mut();
        Some(unsent_queue.front()?.len())
    }

    // Update our send window to the value advertised by our peer.
    //
    pub fn update_send_window(&mut self, header: &TcpHeader) {
        // Check that the ACK we're using to update the window isn't older than the last one used to update it.
        if self.send_window_last_update_seq.get() < header.seq_num
            || (self.send_window_last_update_seq.get() == header.seq_num
                && self.send_window_last_update_ack.get() <= header.ack_num)
        {
            // Update our send window.
            self.send_window.set((header.window_size as u32) << self.window_scale);
            self.send_window_last_update_seq.set(header.seq_num);
            self.send_window_last_update_ack.set(header.ack_num);
        }

        debug!(
            "Updating window size -> {} (hdr {}, scale {})",
            self.send_window.get(),
            header.window_size,
            self.window_scale
        );
    }

    pub fn remote_mss(&self) -> usize {
        self.mss
    }
}
