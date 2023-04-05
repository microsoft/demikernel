// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::super::{
    ctrlblk::ControlBlock,
    sender::UnackedSegment,
};
use crate::{
    inetstack::protocols::tcp::{
        segment::TcpHeader,
        SeqNumber,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};
use ::futures::FutureExt;
use ::std::{
    cmp,
    rc::Rc,
    time::Duration,
};

pub async fn sender(cb: Rc<ControlBlock>) -> Result<!, Fail> {
    'top: loop {
        // First, check to see if there's any unsent data.
        // TODO: Change this to just look at the unsent queue to see if it is empty or not.
        let (unsent_seq, unsent_seq_changed) = cb.get_unsent_seq_no();
        futures::pin_mut!(unsent_seq_changed);

        let (send_next, send_next_changed) = cb.get_send_next();
        futures::pin_mut!(send_next_changed);

        if send_next == unsent_seq {
            futures::select_biased! {
                _ = unsent_seq_changed => continue 'top,
                _ = send_next_changed => continue 'top,
            }
        }

        // Okay, we know we have some unsent data past this point. Next, check to see that the
        // remote side has available window.
        let (win_sz, win_sz_changed) = cb.get_send_window();
        futures::pin_mut!(win_sz_changed);

        // If we don't have any window size at all, we need to transition to PERSIST mode and
        // repeatedly send window probes until window opens up.
        if win_sz == 0 {
            // Send a window probe (this is a one-byte packet designed to elicit a window update from our peer).
            let remote_link_addr = cb.arp().query(cb.get_remote().ip().clone()).await?;
            let buf: DemiBuffer = cb
                .pop_one_unsent_byte()
                .unwrap_or_else(|| panic!("No unsent data? {}, {}", send_next, unsent_seq));

            // Update SND.NXT.
            cb.modify_send_next(|s| s + SeqNumber::from(1));

            // Add the probe byte (as a new separate buffer) to our unacknowledged queue.
            let unacked_segment = UnackedSegment {
                bytes: buf.clone(),
                initial_tx: Some(cb.clock.now()),
            };
            cb.push_unacked_segment(unacked_segment);

            let mut header: TcpHeader = cb.tcp_header();
            header.seq_num = send_next;
            cb.emit(header, Some(buf.clone()), remote_link_addr);

            // Note that we loop here *forever*, exponentially backing off.
            // TODO: Use the correct PERSIST mode timer here.
            let mut timeout: Duration = Duration::from_secs(1);
            loop {
                futures::select_biased! {
                    _ = win_sz_changed => continue 'top,
                    _ = cb.clock.wait(cb.clock.clone(), timeout).fuse() => {
                        timeout *= 2;
                    }
                }
                // Retransmit our window probe.
                let mut header: TcpHeader = cb.tcp_header();
                header.seq_num = send_next;
                cb.emit(header, Some(buf.clone()), remote_link_addr);
            }
        }

        // The remote window is nonzero, but there still may not be room.
        let (send_unacked, send_unacked_changed) = cb.get_send_unacked();
        futures::pin_mut!(send_unacked_changed);

        // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
        cb.congestion_control_on_cwnd_check_before_send();
        let (cwnd, cwnd_changed) = cb.congestion_control_watch_cwnd();
        futures::pin_mut!(cwnd_changed);

        // The limited transmit algorithm may increase the effective size of cwnd by up to 2 * mss.
        let (ltci, ltci_changed) = cb.congestion_control_watch_limited_transmit_cwnd_increase();
        futures::pin_mut!(ltci_changed);

        let effective_cwnd: u32 = cwnd + ltci;
        let next_buf_size: usize = cb.unsent_top_size().expect("no buffer in unsent queue");

        let sent_data: u32 = (send_next - send_unacked).into();
        if win_sz <= (sent_data + next_buf_size as u32)
            || effective_cwnd <= sent_data
            || (effective_cwnd - sent_data) <= cb.get_mss() as u32
        {
            futures::select_biased! {
                _ = send_unacked_changed => continue 'top,
                _ = send_next_changed => continue 'top,
                _ = win_sz_changed => continue 'top,
                _ = cwnd_changed => continue 'top,
                _ = ltci_changed => continue 'top,
            }
        }

        // Past this point we have data to send and it's valid to send it!

        // TODO: Nagle's algorithm - We need to coalese small buffers together to send MSS sized packets.
        // TODO: Silly window syndrome - See RFC 1122's discussion of the SWS avoidance algorithm.

        // TODO: Link-level concerns don't belong here, we should call an IP-level send routine below.
        let remote_link_addr = cb.arp().query(cb.get_remote().ip().clone()).await?;

        // Form an outgoing packet.
        let max_size: usize = cmp::min(
            cmp::min((win_sz - sent_data) as usize, cb.get_mss()),
            (effective_cwnd - sent_data) as usize,
        );
        let segment_data: DemiBuffer = cb
            .pop_unsent_segment(max_size)
            .expect("No unsent data with sequence number gap?");
        let mut segment_data_len: u32 = segment_data.len() as u32;

        let rto: Duration = cb.rto();
        cb.congestion_control_on_send(rto, sent_data);

        // Prepare the segment and send it.
        let mut header: TcpHeader = cb.tcp_header();
        header.seq_num = send_next;
        if segment_data_len == 0 {
            // This buffer is the end-of-send marker.
            debug_assert!(cb.user_is_done_sending.get());
            // Set FIN and adjust sequence number consumption accordingly.
            header.fin = true;
            segment_data_len = 1;
        }
        cb.emit(header, Some(segment_data.clone()), remote_link_addr);

        // Update SND.NXT.
        cb.modify_send_next(|s| s + SeqNumber::from(segment_data_len));

        // Put this segment on the unacknowledged list.
        let unacked_segment = UnackedSegment {
            bytes: segment_data,
            initial_tx: Some(cb.clock.now()),
        };
        cb.push_unacked_segment(unacked_segment);

        // Set the retransmit timer.
        // TODO: Fix how the retransmit timer works.
        let retransmit_deadline = cb.get_retransmit_deadline();
        if retransmit_deadline.is_none() {
            let rto: Duration = cb.rto();
            cb.set_retransmit_deadline(Some(cb.clock.now() + rto));
        }
    }
}
