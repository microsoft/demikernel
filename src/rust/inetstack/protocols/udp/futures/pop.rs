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
        QDesc,
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
    /// Associated queue descriptor.
    qd: QDesc,
    /// Shared receiving queue.
    recv_queue: SharedQueue<SharedQueueSlot<DemiBuffer>>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptor
impl UdpPopFuture {
    /// Creates a pop operation descritor.
    pub fn new(qd: QDesc, recv_queue: SharedQueue<SharedQueueSlot<DemiBuffer>>) -> Self {
        Self { qd, recv_queue }
    }

    /// Returns the queue descriptor that is associated to the target pop operation descriptor.
    pub fn get_qd(&self) -> QDesc {
        self.qd
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
        match self.get_mut().recv_queue.try_pop() {
            Ok(Some(msg)) => Poll::Ready(Ok((msg.remote, msg.data))),
            Ok(None) => {
                let waker: &Waker = ctx.waker();
                waker.wake_by_ref();
                Poll::Pending
            },
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
