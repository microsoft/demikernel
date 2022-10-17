// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
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
use ::windows::Win32::Networking::WinSock::{
    WSAEALREADY,
    WSAEINPROGRESS,
    WSAEWOULDBLOCK,
};

//==============================================================================
// Structures
//==============================================================================

/// Connect Operation Descriptor
pub struct ConnectFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    // Underlying socket.
    socket: Rc<RefCell<Socket>>,
    /// Destination address.
    addr: SockAddr,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Connect Operation Descriptors
impl ConnectFuture {
    /// Creates a descriptor for a connect operation.
    pub fn new(qd: QDesc, socket: Rc<RefCell<Socket>>, addr: SockAddr) -> Self {
        Self { qd, socket, addr }
    }

    /// Returns the queue descriptor associated to the target [ConnectFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Connect Operation Descriptors
impl Future for ConnectFuture {
    type Output = Result<(), Fail>;

    /// Polls the target [ConnectFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut ConnectFuture = self.get_mut();
        match self_.socket.borrow().connect(&self_.addr) {
            // Operation completed.
            Ok(_) => {
                trace!("connection established ({:?})", self_.addr);
                Poll::Ready(Ok(()))
            },
            Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                // Same as OK(_), this happens because establishing a connection may take some time.
                trace!("connection established ({:?})", self_.addr);
                Poll::Ready(Ok(()))
            },
            // Operation not ready yet.
            Err(e) if e.raw_os_error() == Some(WSAEINPROGRESS.0) || e.raw_os_error() == Some(WSAEALREADY.0) => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Operation failed.
            Err(e) => {
                warn!("failed to establish connection ({:?})", e);
                Poll::Ready(Err(Fail::new(e.kind() as i32, "operation failed")))
            },
        }
    }
}
