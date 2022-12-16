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
use ::socket2::Socket;
use ::std::{
    cell::RefCell,
    future::Future,
    mem::{
        transmute,
        MaybeUninit,
    },
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
    // Underlying socket.
    socket: Rc<RefCell<Socket>>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Pop Operation Descriptors
impl PopFuture {
    /// Creates a descriptor for a pop operation.
    pub fn new(qd: QDesc, socket: Rc<RefCell<Socket>>) -> Self {
        Self { qd, socket }
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
    type Output = Result<(Option<SocketAddrV4>, DemiBuffer), Fail>;

    /// Polls the target [PopFuture].
    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut PopFuture = self.get_mut();
        let mut bytes: [MaybeUninit<u8>; POP_SIZE] = MaybeUninit::uninit_array();
        match self_.socket.borrow().recv_from(&mut bytes[..]) {
            // Operation completed.
            Ok((nbytes, socketaddr)) => {
                trace!("data received ({:?}/{:?} bytes)", nbytes, POP_SIZE);
                unsafe {
                    let bytes_recv: [u8; POP_SIZE] = transmute::<[MaybeUninit<u8>; POP_SIZE], [u8; POP_SIZE]>(bytes);
                    let buf: DemiBuffer = DemiBuffer::from_slice(&bytes_recv[0..nbytes])?;
                    Poll::Ready(Ok((socketaddr.as_socket_ipv4(), buf)))
                }
            },
            // Operation in progress.
            Err(e) if e.raw_os_error() == Some(WSAEWOULDBLOCK.0) => {
                ctx.waker().wake_by_ref();
                Poll::Pending
            },
            // Error.
            Err(e) => {
                trace!("pop failed ({:?})", e);
                Poll::Ready(Err(Fail::new(e.kind() as i32, "operation failed")))
            },
        }
    }
}
