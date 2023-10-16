// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::peer::SharedTcpPeer;
use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
    QDesc,
};
use ::std::{
    fmt,
    future::Future,
    net::SocketAddrV4,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

pub struct ConnectFuture<const N: usize> {
    pub qd: QDesc,
    pub peer: SharedTcpPeer<N>,
}

impl<const N: usize> fmt::Debug for ConnectFuture<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ConnectFuture({:?})", self.qd)
    }
}

impl<const N: usize> Future for ConnectFuture<N> {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let mut peer: SharedTcpPeer<N> = self.peer.clone();
        peer.poll_connect_finished(self.qd, context)
    }
}

/// Accept Operation Descriptor
pub struct AcceptFuture<const N: usize> {
    /// Queue descriptor of listening socket.
    qd: QDesc,
    // Pre-booked queue descriptor for incoming connection.
    new_qd: QDesc,
    // Reference to associated inner TCP peer.
    pub peer: SharedTcpPeer<N>,
}

/// Associated Functions for Accept Operation Descriptors
impl<const N: usize> AcceptFuture<N> {
    /// Creates a descriptor for an accept operation.
    pub fn new(qd: QDesc, new_qd: QDesc, peer: SharedTcpPeer<N>) -> Self {
        Self { qd, new_qd, peer }
    }
}

/// Debug Trait Implementation for Accept Operation Descriptors
impl<const N: usize> fmt::Debug for AcceptFuture<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AcceptFuture({:?})", self.qd)
    }
}

/// Future Trait Implementation for Accept Operation Descriptors
impl<const N: usize> Future for AcceptFuture<N> {
    type Output = Result<(QDesc, SocketAddrV4), Fail>;

    /// Polls the underlying accept operation.
    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let mut peer: SharedTcpPeer<N> = self.peer.clone();
        peer.poll_accept(self.qd, self.new_qd, context)
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

pub struct PopFuture<const N: usize> {
    pub qd: QDesc,
    pub size: Option<usize>,
    pub peer: SharedTcpPeer<N>,
}

impl<const N: usize> fmt::Debug for PopFuture<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PopFuture({:?})", self.qd)
    }
}

impl<const N: usize> Future for PopFuture<N> {
    type Output = Result<DemiBuffer, Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut peer: SharedTcpPeer<N> = self.peer.clone();
        peer.poll_recv(self.qd, ctx, self.size)
    }
}

pub struct CloseFuture<const N: usize> {
    pub qd: QDesc,
    pub peer: SharedTcpPeer<N>,
}

impl<const N: usize> fmt::Debug for CloseFuture<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CloseFuture({:?})", self.qd)
    }
}

impl<const N: usize> Future for CloseFuture<N> {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut peer: SharedTcpPeer<N> = self.peer.clone();
        peer.poll_close_finished(self.qd, ctx)
    }
}
