// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::tcp::established::ctrlblk::SharedControlBlock,
    runtime::{
        fail::Fail,
        network::NetworkRuntime,
        scheduler::Yielder,
        timer::SharedTimer,
        watched::SharedWatchedValue,
    },
};
use ::futures::{
    future::{
        self,
        Either,
        FutureExt,
    },
    never::Never,
};
use ::std::time::{
    Duration,
    Instant,
};

pub async fn _retransmitter<N: NetworkRuntime>(mut cb: SharedControlBlock<N>, yielder: Yielder) -> Result<Never, Fail> {
    loop {
        // Pin future for timeout retransmission.
        let mut rtx_deadline_watched: SharedWatchedValue<Option<Instant>> = cb.watch_retransmit_deadline();
        let rtx_yielder: Yielder = Yielder::new();
        let rtx_deadline: Option<Instant> = rtx_deadline_watched.get();
        let rtx_deadline_changed = rtx_deadline_watched.watch(rtx_yielder).fuse();
        futures::pin_mut!(rtx_deadline_changed);
        let clock_ref: SharedTimer = cb.get_timer();
        let rtx_future = match rtx_deadline {
            Some(t) => Either::Left(clock_ref.wait_until(t, &yielder).fuse()),
            None => Either::Right(future::pending()),
        };
        futures::pin_mut!(rtx_future);

        // Pin future for fast retransmission.
        let mut rtx_fast_retransmit_watched: SharedWatchedValue<bool> =
            cb.congestion_control_watch_retransmit_now_flag();
        let retransmit_yielder: Yielder = Yielder::new();
        let rtx_fast_retransmit: bool = rtx_fast_retransmit_watched.get();
        let rtx_fast_retransmit_changed = rtx_fast_retransmit_watched.watch(retransmit_yielder).fuse();
        if rtx_fast_retransmit {
            // Notify congestion control about fast retransmit.
            cb.congestion_control_on_fast_retransmit();

            // Retransmit earliest unacknowledged segment.
            cb.retransmit();
            continue;
        }
        futures::pin_mut!(rtx_fast_retransmit_changed);

        // Since these futures all share a single waker bit, they are all woken whenever one of them triggers.
        futures::select_biased! {
            _ = rtx_deadline_changed => continue,
            _ = rtx_fast_retransmit_changed => continue,
            _ = rtx_future => {
                match cb.get_retransmit_deadline() {
                    Some(timeout) if timeout > cb.get_now() => continue,
                    None => continue,
                    _ => {},
                }

                // Notify congestion control about RTO.
                // TODO: Is this the best place for this?
                // TODO: Why call into ControlBlock to get SND.UNA when congestion_control_on_rto() has access to it?
                let send_unacknowledged = cb.get_send_unacked();
                cb.congestion_control_on_rto(send_unacknowledged.get());

                // RFC 6298 Section 5.4: Retransmit earliest unacknowledged segment.
                cb.retransmit();

                // RFC 6298 Section 5.5: Back off the retransmission timer.
                cb.clone().rto_back_off();

                // RFC 6298 Section 5.6: Restart the retransmission timer with the new RTO.
                let rto: Duration = cb.rto();
                let deadline: Instant = cb.get_now() + rto;
                cb.set_retransmit_deadline(Some(deadline));
            },
        }
    }
}
