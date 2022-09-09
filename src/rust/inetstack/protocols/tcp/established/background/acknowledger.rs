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
use ::std::rc::Rc;

pub async fn acknowledger(cb: Rc<ControlBlock>) -> Result<!, Fail> {
    loop {
        // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
        // - TCP should implement a delayed ACK
        // - The delay must be less than 500ms
        // - For a stream of full-sized segments, there should be an ack for every other segment.

        // TODO: Implement SACKs
        let (ack_deadline, ack_deadline_changed) = cb.get_ack_deadline();
        futures::pin_mut!(ack_deadline_changed);

        let ack_future = match ack_deadline {
            Some(t) => Either::Left(cb.clock.wait_until(cb.clock.clone(), t).fuse()),
            None => Either::Right(future::pending()),
        };
        futures::pin_mut!(ack_future);

        futures::select_biased! {
            _ = ack_deadline_changed => continue,
            _ = ack_future => {
                cb.send_ack();
            },
        }
    }
}
