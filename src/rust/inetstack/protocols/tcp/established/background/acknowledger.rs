// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::SharedControlBlock;
use crate::{
    runtime::{
        fail::Fail,
        timer::SharedTimer,
        watched::SharedWatchedValue,
    },
    scheduler::Yielder,
};
use ::futures::future::{
    self,
    Either,
    FutureExt,
};
use ::std::time::Instant;

pub async fn acknowledger<const N: usize>(mut cb: SharedControlBlock<N>, yielder: Yielder) -> Result<!, Fail> {
    loop {
        // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
        // - TCP should implement a delayed ACK
        // - The delay must be less than 500ms
        // - For a stream of full-sized segments, there should be an ack for every other segment.

        // TODO: Implement SACKs
        let cb2 = cb.clone();
        let mut ack_deadline: SharedWatchedValue<Option<Instant>> = cb2.get_ack_deadline();
        let deadline: Option<Instant> = ack_deadline.get();
        let ack_yielder: Yielder = Yielder::new();
        let ack_deadline_changed = ack_deadline.watch(ack_yielder).fuse();
        futures::pin_mut!(ack_deadline_changed);

        let clock_ref: SharedTimer = cb.get_timer();
        let ack_future = match deadline {
            Some(t) => Either::Left(clock_ref.wait_until(t, &yielder).fuse()),
            None => Either::Right(future::pending()),
        };
        futures::pin_mut!(ack_future);

        futures::select_biased! {
            _ = ack_deadline_changed => continue,
            _ = ack_future => {
                match cb.get_ack_deadline().get() {
                    Some(timeout) if timeout > cb.get_now() => continue,
                    None => continue,
                    _ => {},
                }
                cb.send_ack();
            },
        }
    }
}
