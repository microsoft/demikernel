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
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

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
