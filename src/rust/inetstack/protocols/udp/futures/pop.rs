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
};
use ::std::{
    future::Future,
    net::SocketAddrV4,
    pin::Pin,
    task::{
        Context,
        Poll,
        Waker,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Pop Operation Descriptor
pub struct UdpPopFuture {
    /// Shared receiving queue.
    recv_queue: SharedQueue<SharedQueueSlot<DemiBuffer>>,
    /// Number of bytes to pop.
    size: usize,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptor
impl UdpPopFuture {
    /// Creates a pop operation descritor.
    pub fn new(recv_queue: SharedQueue<SharedQueueSlot<DemiBuffer>>, size: Option<usize>) -> Self {
        const MAX_POP_SIZE: usize = 9000;
        let size: usize = size.unwrap_or(MAX_POP_SIZE);
        Self { recv_queue, size }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait implementation for Pop Operation Descriptor
impl Future for UdpPopFuture {
    type Output = Result<(SocketAddrV4, DemiBuffer), Fail>;

    /// Polls the target pop operation descriptor.
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_: &mut UdpPopFuture = self.get_mut();
        match self_.recv_queue.try_pop() {
            Ok(Some(msg)) => {
                let remote: SocketAddrV4 = msg.remote;
                let mut buf: DemiBuffer = msg.data;
                // We got more bytes than expected, so we trim the buffer.
                if self_.size < buf.len() {
                    buf.trim(self_.size - buf.len())?;
                }
                Poll::Ready(Ok((remote, buf)))
            },
            Ok(None) => {
                let waker: &Waker = ctx.waker();
                waker.wake_by_ref();
                Poll::Pending
            },
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
