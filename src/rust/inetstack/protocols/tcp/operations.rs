// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::peer::{
    Inner,
    TcpPeer,
};
use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    QDesc,
};
use ::std::{
    cell::RefCell,
    fmt,
    future::Future,
    net::SocketAddrV4,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
};

pub struct ConnectFuture {
    pub qd: QDesc,
    pub inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for ConnectFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ConnectFuture({:?})", self.qd)
    }
}

impl Future for ConnectFuture {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        self_.inner.borrow_mut().poll_connect_finished(self_.qd, context)
    }
}

/// Accept Operation Descriptor
pub struct AcceptFuture {
    /// Queue descriptor of listening socket.
    qd: QDesc,
    // Pre-booked queue descriptor for incoming connection.
    new_qd: QDesc,
    // Reference to associated inner TCP peer.
    inner: Rc<RefCell<Inner>>,
}

/// Associated Functions for Accept Operation Descriptors
impl AcceptFuture {
    /// Creates a descriptor for an accept operation.
    pub fn new(qd: QDesc, new_qd: QDesc, inner: Rc<RefCell<Inner>>) -> Self {
        Self { qd, new_qd, inner }
    }
}

/// Debug Trait Implementation for Accept Operation Descriptors
impl fmt::Debug for AcceptFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AcceptFuture({:?})", self.qd)
    }
}

/// Future Trait Implementation for Accept Operation Descriptors
impl Future for AcceptFuture {
    type Output = Result<(QDesc, SocketAddrV4), Fail>;

    /// Polls the underlying accept operation.
    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_: &mut AcceptFuture = self.get_mut();
        // TODO: The following design pattern looks ugly. We should move poll_accept to the inner structure.
        let peer: TcpPeer = TcpPeer {
            inner: self_.inner.clone(),
        };
        peer.poll_accept(self_.qd, self_.new_qd, context)
    }
}

pub struct PushFuture {
    pub qd: QDesc,
    pub err: Option<Fail>,
}

impl fmt::Debug for PushFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PushFuture({:?})", self.qd)
    }
}

impl Future for PushFuture {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context) -> Poll<Self::Output> {
        match self.get_mut().err.take() {
            None => Poll::Ready(Ok(())),
            Some(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct PopFuture {
    pub qd: QDesc,
    pub size: Option<usize>,
    pub inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for PopFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PopFuture({:?})", self.qd)
    }
}

impl Future for PopFuture {
    type Output = Result<DemiBuffer, Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        let size: Option<usize> = self_.size;
        let peer = TcpPeer {
            inner: self_.inner.clone(),
        };
        peer.poll_recv(self_.qd, ctx, size)
    }
}

pub struct CloseFuture {
    pub qd: QDesc,
    pub inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for CloseFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CloseFuture({:?})", self.qd)
    }
}

impl Future for CloseFuture {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        self_.inner.borrow_mut().poll_close_finished(self_.qd, ctx)
    }
}
