// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::nix::{
    errno::Errno,
    sys::{
        socket,
        socket::SockaddrStorage,
    },
};
use ::runtime::{
    fail::Fail,
    memory::{
        Buffer,
        DataBuffer,
    },
    QDesc,
};
use ::std::{
    future::Future,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    os::unix::prelude::RawFd,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

//==============================================================================
// Constants
//==============================================================================

/// Maximum Size for a Pop Operation
const POP_SIZE: usize = 9216;

//==============================================================================
// Structures
//==============================================================================

/// Pop Operation Descriptor
pub struct PopFuture {
    /// Associated queue descriptor.
    qd: QDesc,
    /// Underlying file descriptor.
    fd: RawFd,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Creates a descriptor for a pop operation.
    pub fn new(qd: QDesc, fd: RawFd) -> Self {
        Self { qd, fd }
    }

    /// Returns the queue descriptor associated to the target [PopFuture].
    pub fn get_qd(&self) -> QDesc {
        self.qd
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Future Trait Implementation for Pop Operation Descriptors
impl Future for PopFuture {
    type Output = Result<(Option<SocketAddrV4>, Buffer), Fail>;

    /// Polls the target [PopFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        let mut bytes: [u8; POP_SIZE] = [0; POP_SIZE];
        match socket::recvfrom::<SockaddrStorage>(self_.fd, &mut bytes[..]) {
            // Operation completed.
            Ok((nbytes, socketaddr)) => {
                trace!("data received ({:?}/{:?} bytes)", nbytes, POP_SIZE);
                let buf: Buffer = Buffer::Heap(DataBuffer::from_slice(&bytes[0..nbytes]));
                let addr: Option<SocketAddrV4> = match socketaddr {
                    Some(addr) => match addr.as_sockaddr_in() {
                        Some(sin) => {
                            let ip: Ipv4Addr = Ipv4Addr::from(sin.ip());
                            let port: u16 = sin.port();
                            Some(SocketAddrV4::new(ip, port))
                        },
                        None => None,
                    },
                    _ => None,
                };
                Poll::Ready(Ok((addr, buf)))
            },
            // Operation in progress.
            Err(e) if e == Errno::EWOULDBLOCK || e == Errno::EAGAIN => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Error.
            Err(e) => {
                trace!("pop failed ({:?})", e);
                Poll::Ready(Err(Fail::new(e as i32, "operation failed")))
            },
        }
    }
}
