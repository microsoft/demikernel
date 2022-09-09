// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::ControlBlock;
use crate::inetstack::{
    protocols::tcp::segment::TcpHeader,
    runtime::{
        fail::Fail,
        network::types::MacAddress,
    },
};
use ::futures::{
    future::{
        self,
        Either,
    },
    FutureExt,
};
use ::std::{
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

#[derive(Debug, Eq, PartialEq)]
pub enum RetransmitCause {
    TimeOut,
    FastRetransmit,
}

async fn retransmit(cause: RetransmitCause, cb: &Rc<ControlBlock>) -> Result<(), Fail> {
    // ToDo: Handle retransmission of FIN.

    // ToDo: Fix this routine.  It is currently trashing our unacknowledged queue state.  It shouldn't remove any
    // unacknowledged data from the unacknowledged queue, as retransmiting data doesn't magically make it acknowledged.
    // Any sent data, whether sent once or multiple times, must remain on the unacknowledged queue until it is ACKed.

    // Pop unack'ed segment.
    let mut segment = match cb.pop_unacked_segment() {
        Some(s) => s,
        None => {
            // We shouldn't enter the retransmit routine with an empty unacknowledged queue.  So maybe we should assert
            // here?  But this is relatively benign if it happens, and could be the result of a race-condition or a
            // mismanaged retransmission timer, so asserting would be over-reacting.
            warn!("Retransmission with empty unacknowledged queue");
            if cause == RetransmitCause::TimeOut {
                // Need to cancel the expired timer here, or we could infinite loop.
                cb.set_retransmit_deadline(None);
            }
            return Ok(());
        },
    };

    // TODO: Repacketization - we should send a full MSS.

    // NOTE: Congestion Control Don't think we record a failure on Fast Retransmit, but can't find a definitive source.
    match cause {
        RetransmitCause::TimeOut => cb.rto_record_failure(),
        RetransmitCause::FastRetransmit => (),
    };

    // Our retransmission timer fired, so we need to resend a packet.
    let remote_link_addr: MacAddress = cb.arp().query(cb.get_remote().ip().clone()).await?;

    // Unset the initial timestamp so we don't use this for RTT estimation.
    segment.initial_tx.take();

    // Prepare and send the segment.
    let (seq_no, _) = cb.get_send_unacked();
    let mut header: TcpHeader = cb.tcp_header();
    header.seq_num = seq_no;
    cb.emit(header, Some(segment.bytes), remote_link_addr);

    // Set new retransmit deadline.
    // ToDo: Review this.  Shouldn't we only do this for RetransmitCause::Timeout?
    let rto: Duration = cb.rto_estimate();
    let deadline: Instant = cb.clock.now() + rto;
    cb.set_retransmit_deadline(Some(deadline));

    Ok(())
}

pub async fn retransmitter(cb: Rc<ControlBlock>) -> Result<!, Fail> {
    loop {
        // Pin future for timeout retransmission.
        let (rtx_deadline, rtx_deadline_changed) = cb.watch_retransmit_deadline();
        futures::pin_mut!(rtx_deadline_changed);
        let rtx_future = match rtx_deadline {
            Some(t) => Either::Left(cb.clock.wait_until(cb.clock.clone(), t).fuse()),
            None => Either::Right(future::pending()),
        };
        futures::pin_mut!(rtx_future);

        // Pin future for fast retransmission.
        let (rtx_fast_retransmit, rtx_fast_retransmit_changed) = cb.congestion_control_watch_retransmit_now_flag();
        if rtx_fast_retransmit {
            cb.congestion_control_on_fast_retransmit();
            retransmit(RetransmitCause::FastRetransmit, &cb).await?;
            continue;
        }
        futures::pin_mut!(rtx_fast_retransmit_changed);

        futures::select_biased! {
            _ = rtx_deadline_changed => continue,
            _ = rtx_fast_retransmit_changed => continue,
            _ = rtx_future => {
                trace!("Retransmission Timer Expired");
                let (send_unacknowledged, _) = cb.get_send_unacked();
                cb.congestion_control_on_rto(send_unacknowledged);
                // ToDo: Fix retransmit routine, uncomment next line and delete subsequent line.
                // retransmit(RetransmitCause::TimeOut, &cb).await?;
                cb.set_retransmit_deadline(None);
            },
        }
    }
}
