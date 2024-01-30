// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::SharedControlBlock;
use crate::runtime::{
    conditional_yield_until,
    fail::Fail,
    network::NetworkRuntime,
    watched::SharedWatchedValue,
};
use ::std::time::Instant;

pub async fn acknowledger<N: NetworkRuntime>(mut cb: SharedControlBlock<N>) -> Result<!, Fail> {
    loop {
        // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
        // - TCP should implement a delayed ACK
        // - The delay must be less than 500ms
        // - For a stream of full-sized segments, there should be an ack for every other segment.

        // TODO: Implement SACKs
        let mut ack_deadline: SharedWatchedValue<Option<Instant>> = cb.get_ack_deadline();
        let deadline: Option<Instant> = ack_deadline.get();
        match conditional_yield_until(ack_deadline.watch(), deadline).await {
            Ok(_) => continue,
            Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => cb.send_ack(),
            Err(_) => {
                unreachable!("either the ack deadline changed or the deadline passed, no other errors are possible!")
            },
        }
    }
}
