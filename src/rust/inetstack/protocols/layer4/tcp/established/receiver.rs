// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::{
    collections::{async_queue::AsyncQueue, async_value::SharedAsyncValue},
    expect_ok,
    inetstack::protocols::{
        layer3::SharedLayer3Endpoint,
        layer4::tcp::{
            established::{
                ctrlblk::State, ControlBlock, Sender, MAX_WINDOW_SIZE_WITHOUT_SCALING, MAX_WINDOW_SIZE_WITH_SCALING,
            },
            header::TcpHeader,
            SeqNumber,
        },
    },
    runtime::{fail::Fail, memory::DemiBuffer},
};

use ::futures::never::Never;

//======================================================================================================================
// Constants
//======================================================================================================================

// TODO: Review this value (and its purpose).  It (16 segments) seems awfully small (would make fast retransmit less
// useful), and this mechanism isn't the best way to protect ourselves against deliberate out-of-order segment attacks.
// Ideally, we'd limit out-of-order data to that which (along with the unread data) will fit in the receive window.
const MAX_OUT_OF_ORDER_SIZE_FRAMES: usize = 1024;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct Receiver {
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
    fin_seq_no: SharedAsyncValue<Option<SeqNumber>>,

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

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Receiver {
    pub fn new(
        reader_next_seq_no: SeqNumber,
        receive_next_seq_no: SeqNumber,
        ack_delay_timeout_secs: Duration,
        window_size_bytes: u32,
        window_scale_shift_bits: u8,
    ) -> Self {
        Self {
            reader_next_seq_no,
            receive_next_seq_no,
            fin_seq_no: SharedAsyncValue::new(None),
            pop_queue: AsyncQueue::with_capacity(1024),
            ack_delay_timeout_secs,
            ack_deadline_time_secs: SharedAsyncValue::new(None),
            buffer_size_bytes: window_size_bytes,
            window_scale_shift_bits,
            out_of_order_frames: VecDeque::with_capacity(64),
        }
    }

    // This function causes a EOF to be returned to the user. We also know that there will be no more incoming
    // data after this sequence number.
    fn push_fin(&mut self) {
        self.pop_queue.push(DemiBuffer::new(0));
        debug_assert_eq!(self.receive_next_seq_no, self.fin_seq_no.get().unwrap());
        // Reset it to wake up any close coroutines waiting for FIN to arrive.
        self.fin_seq_no.set(Some(self.receive_next_seq_no));
        // Move RECV_NXT over the FIN.
        self.receive_next_seq_no = self.receive_next_seq_no + 1.into();
    }

    pub fn get_receive_window_size(&self) -> u32 {
        let bytes_unread: u32 = (self.receive_next_seq_no - self.reader_next_seq_no).into();
        // The window should be less than 1GB or 64KB without scaling.
        debug_assert!(
            (self.window_scale_shift_bits == 0 && bytes_unread <= MAX_WINDOW_SIZE_WITHOUT_SCALING)
                || bytes_unread <= MAX_WINDOW_SIZE_WITH_SCALING
        );
        self.buffer_size_bytes - bytes_unread
    }

    pub fn hdr_window_size(&self) -> u16 {
        let window_size: u32 = self.get_receive_window_size();
        let hdr_window_size: u16 = expect_ok!(
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
        // the out-of-order queue is now in-order.  If so, we can move it to the receive queue.
        while !self.out_of_order_frames.is_empty() {
            if let Some(stored_entry) = self.out_of_order_frames.front() {
                if stored_entry.0 == self.receive_next_seq_no {
                    // Move this entry's buffer from the out-of-order store to the receive queue.
                    // This data is now considered to be "received" by TCP, and included in our RCV.NXT calculation.
                    debug!("Recovering out-of-order packet at {}", self.receive_next_seq_no);
                    if let Some(temp) = self.out_of_order_frames.pop_front() {
                        self.receive_next_seq_no = self.receive_next_seq_no + SeqNumber::from(temp.1.len() as u32);
                        // This inserts the segment and wakes a waiting pop coroutine.
                        self.pop_queue.push(temp.1);
                    }
                } else {
                    // Since our out-of-order list is sorted, we can stop when the next segment is not in sequence.
                    break;
                }
            }
        }
    }

    // Block until the remote sends a FIN (plus all previous data has arrived).
    pub async fn wait_for_fin(&mut self) -> Result<(), Fail> {
        let mut fin_seq_no: Option<SeqNumber> = self.fin_seq_no.get();
        loop {
            match fin_seq_no {
                Some(fin_seq_no) if self.receive_next_seq_no >= fin_seq_no => return Ok(()),
                _ => {
                    fin_seq_no = self.fin_seq_no.wait_for_change(None).await?;
                },
            }
        }
    }

    // Block until some data is received, up to an optional size.
    pub async fn pop(&mut self, size: Option<usize>) -> Result<DemiBuffer, Fail> {
        debug!("waiting on pop {:?}", size);
        let buf: DemiBuffer = if let Some(size) = size {
            let mut buf: DemiBuffer = self.pop_queue.pop(None).await?;
            // Split the buffer if it's too big.
            if buf.len() > size {
                buf.split_front(size)?
            } else {
                buf
            }
        } else {
            self.pop_queue.pop(None).await?
        };

        match buf.len() {
            len if len > 0 => {
                self.reader_next_seq_no = self.reader_next_seq_no + SeqNumber::from(buf.len() as u32);
            },
            _ => {
                debug!("found FIN");
                self.reader_next_seq_no = self.reader_next_seq_no + 1.into();
            },
        }

        Ok(buf)
    }

    // Receive a single incoming packet from layer3.
    pub fn receive(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        tcp_hdr: TcpHeader,
        buf: DemiBuffer,
        now: Instant,
    ) {
        match Self::process_packet(cb, layer3_endpoint, tcp_hdr, buf, now) {
            Ok(()) => (),
            Err(e) => debug!("Dropped packet: {:?}", e),
        }
    }

    /// This is the main function for processing an incoming packet during the Established state when the connection is
    /// active. Each step in this function return Ok if there is further processing to be done and EBADMSG if the
    /// packet should be dropped after the step.
    fn process_packet(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
        mut header: TcpHeader,
        mut data: DemiBuffer,
        now: Instant,
    ) -> Result<(), Fail> {
        let mut seg_start: SeqNumber = header.seq_num;
        let mut seg_end: SeqNumber = seg_start;
        let mut seg_len: u32 = data.len() as u32;

        // Check if the segment is in the receive window and trim off everything else.
        Self::check_segment_in_window(
            cb,
            layer3_endpoint,
            &mut header,
            &mut data,
            &mut seg_start,
            &mut seg_end,
            &mut seg_len,
        )?;
        Self::check_and_process_rst(cb, &header)?;
        Self::check_syn(&header)?;
        Self::check_and_process_ack(cb, &header, now)?;

        // TODO: Check the URG bit.  If we decide to support this, how should we do it?
        if header.urg {
            warn!("Got packet with URG bit set!");
        }

        if data.len() > 0 {
            Self::process_data(cb, layer3_endpoint, data, seg_start, seg_end, seg_len)?;
        }

        // Deal with FIN flag, saving the FIN for later if it is out of order.
        if header.fin {
            match cb.receiver.fin_seq_no.get() {
                // We've already received this FIN, so ignore.
                Some(seq_no) if seq_no != seg_end => warn!(
                    "Received a FIN with a different sequence number, ignoring. previous={:?} new={:?}",
                    seq_no, seg_end,
                ),
                Some(_) => trace!("Received duplicate FIN"),
                None => {
                    trace!("Received FIN");
                    cb.receiver.fin_seq_no.set(seg_end.into());
                },
            }
        };

        // Check whether we've received the last packet in this TCP stream.
        if cb
            .receiver
            .fin_seq_no
            .get()
            .is_some_and(|seq_no| seq_no == cb.receiver.receive_next_seq_no)
        {
            // Once we know there is no more data coming, begin closing down the connection.
            Self::process_fin(cb);
        }

        // Send an ack on every FIN. We do this separately here because if the FIN is in order, we ack it after the
        // previous line, otherwise we do not ack the FIN.
        if header.fin {
            trace!("Acking FIN");
            Sender::send_ack(cb, layer3_endpoint)
        }

        // We should ACK this segment, preferably via piggybacking on a response.
        if cb.receiver.ack_deadline_time_secs.get().is_none() {
            // Start the delayed ACK timer to ensure an ACK gets sent soon even if no piggyback opportunity occurs.
            let timeout: Duration = cb.receiver.ack_delay_timeout_secs;
            // Getting the current time is extremely cheap as it is just a variable lookup.
            cb.receiver.ack_deadline_time_secs.set(Some(now + timeout));
        } else {
            // We already owe our peer an ACK (the timer was already running), so cancel the timer and ACK now.
            cb.receiver.ack_deadline_time_secs.set(None);
            trace!("process_packet(): sending ack on deadline expiration");
            Sender::send_ack(cb, layer3_endpoint);
        }

        Ok(())
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

        let receive_next: SeqNumber = cb.receiver.receive_next_seq_no;

        let after_receive_window: SeqNumber = receive_next + SeqNumber::from(cb.receiver.get_receive_window_size());

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
                        Sender::send_ack(cb, layer3_endpoint);
                    }
                    let cause: String = format!("duplicate packet");
                    error!("check_segment_in_window(): {}", cause);
                    return Err(Fail::new(libc::EBADMSG, &cause));
                } else {
                    // Some of this segment's data is new.  Cut the duplicate data off of the front.
                    // If there is a SYN at the start of this segment, remove it too.
                    let mut duplicate: u32 = u32::from(receive_next - *seg_start);
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
                        Sender::send_ack(cb, layer3_endpoint);
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
        cb.state = State::Closed;
        cb.receiver.push_fin();
        return Err(Fail::new(libc::ECONNRESET, "remote reset connection"));
    }

    // Check the SYN bit.
    fn check_syn(header: &TcpHeader) -> Result<(), Fail> {
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
    fn check_and_process_ack(cb: &mut ControlBlock, header: &TcpHeader, now: Instant) -> Result<(), Fail> {
        if !header.ack {
            // All segments on established connections should be ACKs.  Drop this segment.
            let cause: String = format!("Received non-ACK segment on established connection");
            error!("{}", cause);
            return Err(Fail::new(libc::EBADMSG, &cause));
        }

        // TODO: RFC 5961 "Blind Data Injection Attack" prevention would have us perform additional ACK validation
        // checks here.

        Sender::process_ack(cb, header, now);

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
        // We can only process in-order data.  Check for out-of-order segment.
        if seg_start != cb.receiver.receive_next_seq_no {
            debug!("Received out-of-order segment");
            debug_assert_ne!(seg_len, 0);
            // This segment is out-of-order.  If it carries data, we should store it for later processing
            // after the "hole" in the sequence number space has been filled.
            match cb.state {
                State::Established | State::FinWait1 | State::FinWait2 => {
                    debug_assert_eq!(seg_len, data.len() as u32);
                    cb.receiver.store_out_of_order_segment(seg_start, seg_end, data);
                    // Sending an ACK here is only a "MAY" according to the RFCs, but helpful for fast retransmit.
                    trace!("process_data(): send ack on out-of-order segment");
                    Sender::send_ack(cb, layer3_endpoint);
                },
                state => warn!("Ignoring data received after FIN (in state {:?}).", state),
            }

            // We're done with this out-of-order segment.
            return Ok(());
        }

        // We can only legitimately receive data in ESTABLISHED, FIN-WAIT-1, and FIN-WAIT-2.
        cb.receiver.receive_data(seg_start, data);
        Ok(())
    }

    // This routine takes an incoming TCP segment and adds it to the out-of-order receive queue.
    // If the new segment had a FIN it has been removed prior to this routine being called.
    // Note: Since this is not the "fast path", this is written for clarity over efficiency.
    //
    fn store_out_of_order_segment(&mut self, mut new_start: SeqNumber, mut new_end: SeqNumber, mut buf: DemiBuffer) {
        let mut action_index: usize = self.out_of_order_frames.len();
        let mut another_pass_neeeded: bool = true;

        while another_pass_neeeded {
            another_pass_neeeded = false;

            // Find the new segment's place in the out-of-order store.
            // The out-of-order store is sorted by starting sequence number, and contains no duplicate data.
            action_index = self.out_of_order_frames.len();
            for index in 0..self.out_of_order_frames.len() {
                let stored_segment: &(SeqNumber, DemiBuffer) = &self.out_of_order_frames[index];

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
                    let duplicate: u32 = u32::from(stored_end - new_start);
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

    fn process_fin(cb: &mut ControlBlock) {
        let state = match cb.state {
            State::Established => State::CloseWait,
            State::FinWait1 => State::Closing,
            State::FinWait2 => State::TimeWait,
            state => unreachable!("Cannot be in any other state at this point: {:?}", state),
        };
        cb.state = state;
        cb.receiver.push_fin();
    }

    pub async fn acknowledger(
        cb: &mut ControlBlock,
        layer3_endpoint: &mut SharedLayer3Endpoint,
    ) -> Result<Never, Fail> {
        let mut ack_deadline: SharedAsyncValue<Option<Instant>> = cb.receiver.ack_deadline_time_secs.clone();
        let mut deadline: Option<Instant> = ack_deadline.get();
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
                    Sender::send_ack(cb, layer3_endpoint);
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
