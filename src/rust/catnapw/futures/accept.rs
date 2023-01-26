// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    fail::Fail,
    QDesc,
};
use ::socket2::Socket;
use ::std::{
    cell::RefCell,
    future::Future,
    net::SocketAddrV4,
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

/// Accept Operation Descriptor
pub struct AcceptFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Underlying socket.
    socket: Rc<RefCell<Socket>>,
    /// Queue descriptor of incoming connection.
    new_qd: QDesc,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Accept Operation Descriptors
impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(qd: QDesc, socket: Rc<RefCell<Socket>>, new_qd: QDesc) -> Self {
        Self { qd, socket, new_qd }
    }

    /// Returns the queue descriptor associated to the target [AcceptFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }

    /// Returns the new queue descriptor associated to the target [AcceptFuture].
    pub fn get_new_qd(&self) -> QDesc {
        self.new_qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Accept Operation Descriptors
impl Future for AcceptFuture {
    type Output = Result<(Socket, SocketAddrV4), Fail>;

    /// Polls the target [AcceptFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &AcceptFuture = self.get_mut();
        match self_.socket.borrow().accept() {
            // Operation completed.
            Ok((new_socket, saddr)) => {
                trace!("connection accepted ({:?})", new_socket);

                // Set async options in socket.
                match new_socket.set_nodelay(true) {
                    Ok(_) => {},
                    Err(_) => warn!("cannot set TCP_NONDELAY option"),
                }
                match new_socket.set_nonblocking(true) {
                    Ok(_) => {},
                    Err(_) => warn!("cannot set NONBLOCK option"),
                };
                // It is ok to have the expect() statement bellow because if
                // this is not a SocketAddrV4 something really bad happen.
                let addr: SocketAddrV4 = saddr.as_socket_ipv4().expect("not a SocketAddrV4");
                Poll::Ready(Ok((new_socket, addr)))
            },
            // Operation in progress.
            Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Operation failed.
            Err(e) => {
                warn!("failed to accept connection ({:?})", e);
                Poll::Ready(Err(Fail::new(e.kind() as i32, "operation failed")))
            },
        }
    }
}
