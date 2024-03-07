// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    collections::async_value::SharedAsyncValue,
    inetstack::protocols::tcp::established::ctrlblk::SharedControlBlock,
    runtime::{
        conditional_yield_until,
        fail::Fail,
        network::NetworkRuntime,
    },
};
use ::futures::{
    never::Never,
    pin_mut,
    select_biased,
    FutureExt,
};
use ::std::time::{
    Duration,
    Instant,
};

pub async fn retransmitter<N: NetworkRuntime>(mut cb: SharedControlBlock<N>) -> Result<Never, Fail> {
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
            cb.retransmit();
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
            Err(_) => {
                unreachable!(
                    "either the retransmit deadline changed or the deadline passed, no other errors are possible!"
                )
            },
        }
    }
}
