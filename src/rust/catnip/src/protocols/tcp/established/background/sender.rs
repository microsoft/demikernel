use super::super::state::{
    sender::UnackedSegment,
    ControlBlock,
};
use crate::{
    fail::Fail,
    runtime::Runtime,
};
use futures::FutureExt;
use std::{
    cmp,
    num::Wrapping,
    rc::Rc,
    time::Duration,
};

pub async fn sender<RT: Runtime>(cb: Rc<ControlBlock<RT>>) -> Result<!, Fail> {
    'top: loop {
        // First, check to see if there's any unsent data.
        let (unsent_seq, unsent_seq_changed) = cb.sender.unsent_seq_no.watch();
        futures::pin_mut!(unsent_seq_changed);

        // TODO: We don't need to watch this value since we're the only mutator.
        let (sent_seq, sent_seq_changed) = cb.sender.sent_seq_no.watch();
        futures::pin_mut!(sent_seq_changed);

        if sent_seq == unsent_seq {
            futures::select_biased! {
                _ = unsent_seq_changed => continue 'top,
                _ = sent_seq_changed => continue 'top,
            }
        }

        // Okay, we know we have some unsent data past this point. Next, check to see that the
        // remote side has available window.
        let (win_sz, win_sz_changed) = cb.sender.window_size.watch();
        futures::pin_mut!(win_sz_changed);

        // If we don't have any window size at all, we need to transition to PERSIST state and
        // repeatedly send window probes until window opens up.
        if win_sz == 0 {
            let remote_link_addr = cb.arp.query(cb.remote.address()).await?;
            let buf = cb
                .sender
                .pop_one_unsent_byte()
                .unwrap_or_else(|| panic!("No unsent data? {}, {}", sent_seq, unsent_seq));

            cb.sender.sent_seq_no.modify(|s| s + Wrapping(1));
            let unacked_segment = UnackedSegment {
                bytes: buf.clone(),
                initial_tx: Some(cb.rt.now()),
            };
            cb.sender
                .unacked_queue
                .borrow_mut()
                .push_back(unacked_segment);

            let mut header = cb.tcp_header();
            header.seq_num = sent_seq;
            cb.emit(header, buf.clone(), remote_link_addr);

            // Note that we loop here *forever*, exponentially backing off.
            // TODO: Use the correct PERSIST state timer here.
            let mut timeout = Duration::from_secs(1);
            loop {
                futures::select_biased! {
                    _ = win_sz_changed => continue 'top,
                    _ = cb.rt.wait(timeout).fuse() => {
                        timeout *= 2;
                    }
                }
                // Retransmit our window probe.
                let mut header = cb.tcp_header();
                header.seq_num = sent_seq;
                cb.emit(header, buf.clone(), remote_link_addr);
            }
        }

        // The remote window is nonzero, but there still may not be room.
        let (base_seq, base_seq_changed) = cb.sender.base_seq_no.watch();
        futures::pin_mut!(base_seq_changed);

        let Wrapping(sent_data) = sent_seq - base_seq;
        if win_sz <= sent_data {
            futures::select_biased! {
                _ = base_seq_changed => continue 'top,
                _ = sent_seq_changed => continue 'top,
                _ = win_sz_changed => continue 'top,
            }
        }

        // TODO: Nagle's algorithm
        // TODO: Silly window syndrome
        let remote_link_addr = cb.arp.query(cb.remote.address()).await?;

        // Form an outgoing packet.
        let max_size = cmp::min((win_sz - sent_data) as usize, cb.sender.mss);
        let segment_data = cb
            .sender
            .pop_unsent(max_size)
            .expect("No unsent data with sequence number gap?");
        let segment_data_len = segment_data.len();
        assert!(segment_data_len > 0);

        let mut header = cb.tcp_header();
        header.seq_num = sent_seq;
        cb.emit(header, segment_data.clone(), remote_link_addr);

        cb.sender
            .sent_seq_no
            .modify(|s| s + Wrapping(segment_data_len as u32));
        let unacked_segment = UnackedSegment {
            bytes: segment_data,
            initial_tx: Some(cb.rt.now()),
        };
        cb.sender
            .unacked_queue
            .borrow_mut()
            .push_back(unacked_segment);

        if cb.sender.retransmit_deadline.get().is_none() {
            let rto = cb.sender.rto.borrow().estimate();
            cb.sender.retransmit_deadline.set(Some(cb.rt.now() + rto));
        }
    }
}
