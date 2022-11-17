// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    QDesc,
};
use ::socket2::{
    SockAddr,
    Socket,
};
use ::std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
};
use ::windows::Win32::Networking::WinSock::WSAEWOULDBLOCK;

//==============================================================================
// Structures
//==============================================================================

/// Pushto Operation Descriptor
pub struct PushtoFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Destination address.
    addr: SockAddr,
    // Underlying socket.
    socket: Rc<RefCell<Socket>>,
    /// Buffer to send.
    buf: DemiBuffer,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pushto Operation Descriptors
impl PushtoFuture {
    /// Creates a descriptor for a pushto operation.
    pub fn new(qd: QDesc, socket: Rc<RefCell<Socket>>, addr: SockAddr, buf: DemiBuffer) -> Self {
        Self { qd, addr, socket, buf }
    }

    /// Returns the queue descriptor associated to the target [PushtoFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Pushto Operation Descriptors
impl Future for PushtoFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [PushtoFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PushtoFuture = self.get_mut();
        match self_.socket.borrow().send_to(&self_.buf[..], &self_.addr) {
            // Operation completed.
            Ok(nbytes) => {
                trace!("data pushed ({:?}/{:?} bytes)", nbytes, self_.buf.len());
                Poll::Ready(Ok(()))
            },
            // Operation in progress.
            Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Error.
            Err(e) => {
                warn!("push failed ({:?})", e);
                Poll::Ready(Err(Fail::new(e.kind() as i32, "operation failed")))
            },
        }
    }
}
