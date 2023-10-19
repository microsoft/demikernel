// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::udp::queue::{
        SharedQueue,
        SharedQueueSlot,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
    scheduler::Yielder,
};
use ::std::net::SocketAddrV4;

pub async fn udp_pop_coroutine(
    recv_queue: SharedQueue<SharedQueueSlot<DemiBuffer>>,
    size: Option<usize>,
    yielder: Yielder,
) -> Result<(SocketAddrV4, DemiBuffer), Fail> {
    const MAX_POP_SIZE: usize = 9000;
    let size: usize = size.unwrap_or(MAX_POP_SIZE);

    loop {
        match recv_queue.try_pop() {
            Ok(Some(msg)) => {
                let remote: SocketAddrV4 = msg.remote;
                let mut buf: DemiBuffer = msg.data;
                // We got more bytes than expected, so we trim the buffer.
                if size < buf.len() {
                    buf.trim(size - buf.len())?;
                };
                return Ok((remote, buf));
            },
            Ok(None) => match yielder.yield_once().await {
                Ok(()) => continue,
                Err(e) => return Err(e),
            },
            Err(e) => return Err(e),
        }
    }
}
