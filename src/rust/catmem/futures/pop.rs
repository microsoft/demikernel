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

/// Pop Operation Descriptor
pub struct PopFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Underlying shared ring buffer.
    ring: Rc<SharedRingBuffer<u16>>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Maximum Size for a Pop Operation
    const POP_SIZE_MAX: usize = 9216;

    /// Creates a descriptor for a pop operation.
    pub fn new(qd: QDesc, ring: Rc<SharedRingBuffer<u16>>) -> Self {
        PopFuture { qd, ring }
    }

    /// Returns the queue descriptor associated to the target [PopFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Future Trait Implementation for Pop Operation Descriptors
impl Future for PopFuture {
    type Output = Result<(DemiBuffer, bool), Fail>;

    /// Polls the target [PopFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        let mut buf: DemiBuffer = DemiBuffer::new(Self::POP_SIZE_MAX as u16);
        let mut eof: bool = false;
        let mut index: usize = 0;
        loop {
            match self_.ring.try_dequeue() {
                Some(x) => {
                    let (high, low): (u8, u8) = (((x >> 8) & 0xff) as u8, (x & 0xff) as u8);
                    if high != 0 {
                        buf.trim(Self::POP_SIZE_MAX - index)
                            .expect("cannot trim more bytes than the buffer has");
                        eof = true;
                        break;
                    } else {
                        buf[index] = low;
                        index += 1;
                    }
                },
                None => {
                    if index > 0 {
                        trace!("data read ({:?}/{:?} bytes)", index, Self::POP_SIZE_MAX);
                        buf.trim(Self::POP_SIZE_MAX - index)
                            .expect("cannot trim more bytes than the buffer has");
                        break;
                    } else {
                        ctx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                },
            }
        }
        Poll::Ready(Ok((buf, eof)))
    }
}
