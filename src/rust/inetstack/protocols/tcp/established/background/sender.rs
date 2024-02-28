// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::tcp::{
        established::{
            ctrlblk::SharedControlBlock,
            sender::UnackedSegment,
        },
        segment::TcpHeader,
        SeqNumber,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::NetworkRuntime,
        scheduler::Yielder,
        timer::SharedTimer,
        watched::SharedWatchedValue,
    },
};
use ::futures::{
    never::Never,
    FutureExt,
};
use ::std::{
    cmp,
    time::Duration,
};

pub async fn _sender<N: NetworkRuntime>(mut cb: SharedControlBlock<N>, yielder: Yielder) -> Result<Never, Fail> {
    'top: loop {
        // First, check to see if there's any unsent data.
        // TODO: Change this to just look at the unsent queue to see if it is empty or not.
        let cb2 = cb.clone();
        let mut unsent_seq_watched: SharedWatchedValue<SeqNumber> = cb2.get_unsent_seq_no();
        let unsent_seq: SeqNumber = unsent_seq_watched.get();
        let unsent_yielder: Yielder = Yielder::new();
        let unsent_seq_changed = unsent_seq_watched.watch(unsent_yielder).fuse();
        futures::pin_mut!(unsent_seq_changed);

        let cb3 = cb.clone();
        let mut send_next_watched: SharedWatchedValue<SeqNumber> = cb3.get_send_next();
        let send_next: SeqNumber = send_next_watched.get();
        let send_yielder: Yielder = Yielder::new();
        let send_next_changed = send_next_watched.watch(send_yielder).fuse();
        futures::pin_mut!(send_next_changed);

        if send_next == unsent_seq {
            futures::select_biased! {
                _ = unsent_seq_changed => continue 'top,
                _ = send_next_changed => continue 'top,
            }
        }

        // Okay, we know we have some unsent data past this point. Next, check to see that the
        // remote side has available window.
        let mut win_sz_watched: SharedWatchedValue<u32> = cb.get_send_window();
        let win_sz: u32 = win_sz_watched.get();
        let win_sz_yielder: Yielder = Yielder::new();
        let win_sz_changed = win_sz_watched.watch(win_sz_yielder).fuse();
        futures::pin_mut!(win_sz_changed);

        // If we don't have any window size at all, we need to transition to PERSIST mode and
        // repeatedly send window probes until window opens up.
        if win_sz == 0 {
            // Send a window probe (this is a one-byte packet designed to elicit a window update from our peer).
            let arp_yielder: Yielder = Yielder::new();
            let remote_link_addr = cb.arp().query(cb.get_remote().ip().clone(), &arp_yielder).await?;
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

            let mut header: TcpHeader = cb.tcp_header();
            header.seq_num = send_next;
            let mut cb4 = cb.clone();
            cb4.emit(header, Some(buf.clone()), remote_link_addr);

            // Note that we loop here *forever*, exponentially backing off.
            // TODO: Use the correct PERSIST mode timer here.
            let mut timeout: Duration = Duration::from_secs(1);
            loop {
                let clock_ref: SharedTimer = cb.get_timer();

                futures::select_biased! {
                    _ = win_sz_changed => continue 'top,
                    _ = clock_ref.wait(timeout, &yielder).fuse() => {
                        timeout *= 2;
                    }
                }
                // Retransmit our window probe.
                let mut header: TcpHeader = cb.tcp_header();
                header.seq_num = send_next;
                let mut cb4 = cb.clone();
                cb4.emit(header, Some(buf.clone()), remote_link_addr);
            }
        }

        // The remote window is nonzero, but there still may not be room.
        let mut send_unacked_watched: SharedWatchedValue<SeqNumber> = cb.get_send_unacked();
        let send_unacked: SeqNumber = send_unacked_watched.get();
        let send_unacked_yielder: Yielder = Yielder::new();
        let send_unacked_changed = send_unacked_watched.watch(send_unacked_yielder).fuse();
        futures::pin_mut!(send_unacked_changed);

        // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle.
        cb.congestion_control_on_cwnd_check_before_send();
        let mut cwnd_watched: SharedWatchedValue<u32> = cb.congestion_control_get_cwnd();
        let cwnd: u32 = cwnd_watched.get();
        let cwnd_yielder: Yielder = Yielder::new();
        let cwnd_changed = cwnd_watched.watch(cwnd_yielder).fuse();
        futures::pin_mut!(cwnd_changed);

        // The limited transmit algorithm may increase the effective size of cwnd by up to 2 * mss.
        let mut ltci_watched: SharedWatchedValue<u32> = cb.congestion_control_get_limited_transmit_cwnd_increase();
        let ltci: u32 = ltci_watched.get();
        let ltci_yielder: Yielder = Yielder::new();
        let ltci_changed = ltci_watched.watch(ltci_yielder).fuse();
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
        let arp_yielder: Yielder = Yielder::new();
        let remote_link_addr = cb.arp().query(cb.get_remote().ip().clone(), &arp_yielder).await?;

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
