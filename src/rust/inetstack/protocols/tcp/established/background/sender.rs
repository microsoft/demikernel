// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    collections::async_value::SharedAsyncValue,
    inetstack::protocols::tcp::{
        established::{
            ctrlblk::SharedControlBlock,
            sender::UnackedSegment,
        },
        segment::TcpHeader,
        SeqNumber,
    },
    runtime::{
        conditional_yield_until,
        fail::Fail,
        memory::DemiBuffer,
        network::NetworkRuntime,
    },
};
use ::futures::{
    never::Never,
    pin_mut,
    select_biased,
    FutureExt,
};
use ::std::{
    cmp,
    pin::pin,
    time::Duration,
};

pub async fn sender<N: NetworkRuntime>(mut cb: SharedControlBlock<N>) -> Result<Never, Fail> {
    'top: loop {
        // First, check to see if there's any unsent data.
        // TODO: Change this to just look at the unsent queue to see if it is empty or not.
        let mut unsent_seq_watched: SharedAsyncValue<SeqNumber> = cb.get_unsent_seq_no();
        let unsent_seq: SeqNumber = unsent_seq_watched.get();

        let mut send_next_watched: SharedAsyncValue<SeqNumber> = cb.get_send_next();
        let send_next: SeqNumber = send_next_watched.get();

        if send_next == unsent_seq {
            let something_changed = async move {
                select_biased! {
                    _ = unsent_seq_watched.wait_for_change(None).fuse() => (),
                    _ = send_next_watched.wait_for_change(None).fuse() => (),
                }
            };
            pin_mut!(something_changed);
            match conditional_yield_until(something_changed, None).await {
                Ok(()) => continue 'top,
                Err(_) => {
                    unreachable!("either the sent or unsent sequence number changed, no other errors are possible!")
                },
            };
        }

        // Okay, we know we have some unsent data past this point. Next, check to see that the
        // remote side has available window.
        let mut win_sz_watched: SharedAsyncValue<u32> = cb.get_send_window();
        let win_sz: u32 = win_sz_watched.get();

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
                initial_tx: Some(cb.get_now()),
            };
            cb.push_unacked_segment(unacked_segment);

            // Note that we loop here *forever*, exponentially backing off.
            // TODO: Use the correct PERSIST mode timer here.
            let mut timeout: Duration = Duration::from_secs(1);
            loop {
                // Create packet.
                let mut header: TcpHeader = cb.tcp_header();
                header.seq_num = send_next;
                cb.emit(header, Some(buf.clone()), remote_link_addr);

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
        let mut send_unacked_watched: SharedAsyncValue<SeqNumber> = cb.get_send_unacked();
        let send_unacked: SeqNumber = send_unacked_watched.get();

        // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
        cb.congestion_control_on_cwnd_check_before_send();
        let mut cwnd_watched: SharedAsyncValue<u32> = cb.congestion_control_get_cwnd();
        let cwnd: u32 = cwnd_watched.get();

        // The limited transmit algorithm may increase the effective size of cwnd by up to 2 * mss.
        let mut ltci_watched: SharedAsyncValue<u32> = cb.congestion_control_get_limited_transmit_cwnd_increase();
        let ltci: u32 = ltci_watched.get();

        let effective_cwnd: u32 = cwnd + ltci;
        let next_buf_size: usize = cb.unsent_top_size().expect("no buffer in unsent queue");

        let sent_data: u32 = (send_next - send_unacked).into();
        if win_sz <= (sent_data + next_buf_size as u32)
            || effective_cwnd <= sent_data
            || (effective_cwnd - sent_data) <= cb.get_mss() as u32
        {
            futures::select_biased! {
                _ = pin!(send_unacked_watched.wait_for_change(None).fuse()) => continue 'top,
                _ = send_next_watched.wait_for_change(None).fuse() => continue 'top,
                _ = win_sz_watched.wait_for_change(None).fuse() => continue 'top,
                _ = cwnd_watched.wait_for_change(None).fuse() => continue 'top,
                _ = ltci_watched.wait_for_change(None).fuse() => continue 'top,
            };
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
        let (segment_data, do_push): (DemiBuffer, bool) = cb
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
            // Set FIN and adjust sequence number consumption accordingly.
            header.fin = true;
            segment_data_len = 1;
        } else if do_push {
            header.psh = true;
        }
        let mut cb4 = cb.clone();
        cb4.emit(header, Some(segment_data.clone()), remote_link_addr);

        // Update SND.NXT.
        cb.modify_send_next(|s| s + SeqNumber::from(segment_data_len));

        // Put this segment on the unacknowledged list.
        let unacked_segment = UnackedSegment {
            bytes: segment_data,
            initial_tx: Some(cb.get_now()),
        };
        cb.push_unacked_segment(unacked_segment);

        // Set the retransmit timer.
        // TODO: Fix how the retransmit timer works.
        let retransmit_deadline = cb.get_retransmit_deadline();
        if retransmit_deadline.is_none() {
            let rto: Duration = cb.rto();
            cb.set_retransmit_deadline(Some(cb.get_now() + rto));
        }
    }
}
