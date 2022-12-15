// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::ControlBlock;
use crate::runtime::fail::Fail;
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
            // Notify congestion control about fast retransmit.
            cb.congestion_control_on_fast_retransmit();

            // Retransmit earliest unacknowledged segment.
            cb.retransmit();
            continue;
        }
        futures::pin_mut!(rtx_fast_retransmit_changed);

        futures::select_biased! {
            _ = rtx_deadline_changed => continue,
            _ = rtx_fast_retransmit_changed => continue,
            _ = rtx_future => {
                trace!("Retransmission Timer Expired");
                // Notify congestion control about RTO.
                // ToDo: Is this the best place for this?
                // ToDo: Why call into ControlBlock to get SND.UNA when congestion_control_on_rto() has access to it?
                let (send_unacknowledged, _) = cb.get_send_unacked();
                cb.congestion_control_on_rto(send_unacknowledged);

                // RFC 6298 Section 5.4: Retransmit earliest unacknowledged segment.
                cb.retransmit();

                // RFC 6298 Section 5.5: Back off the retransmission timer.
                cb.rto_back_off();

                // RFC 6298 Section 5.6: Restart the retransmission timer with the new RTO.
                let rto: Duration = cb.rto();
                let deadline: Instant = cb.clock.now() + rto;
                cb.set_retransmit_deadline(Some(deadline));
            },
        }
    }
}
