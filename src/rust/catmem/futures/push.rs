// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catmem::SharedRingBuffer,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
    QDesc,
};
use ::std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Push Operation Descriptor
pub struct PushFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Write index on the underlying shared ring buffer.
    index: usize,
    // Underlying shared ring buffer.
    ring: Rc<SharedRingBuffer<u16>>,
    /// Buffer to send.
    buf: DemiBuffer,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Push Operation Descriptors
impl PushFuture {
    /// Creates a descriptor for a push operation.
    pub fn new(qd: QDesc, ring: Rc<SharedRingBuffer<u16>>, buf: DemiBuffer) -> Self {
        PushFuture {
            qd,
            ring,
            index: 0,
            buf,
        }
    }

    /// Returns the queue descriptor associated to the target [PushFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Future Trait Implementation for Push Operation Descriptors
impl Future for PushFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [PushFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PushFuture = self.get_mut();
        let mut index: usize = self_.index;
        for low in &self_.buf[index..] {
            let x: u16 = (low & 0xff) as u16;
            match self_.ring.try_enqueue(x) {
                Ok(()) => index += 1,
                Err(_) => {
                    self_.index = index;
                    ctx.waker().wake_by_ref();
                    return Poll::Pending;
                },
            }
        }
        trace!("data written ({:?}/{:?} bytes)", index, self_.buf.len());
        Poll::Ready(Ok(()))
    }
}
