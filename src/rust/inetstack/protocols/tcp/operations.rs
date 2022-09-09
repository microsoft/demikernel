// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::peer::{
    Inner,
    TcpPeer,
};
use crate::{
    inetstack::operations::OperationResult,
    runtime::{
        fail::Fail,
        memory::Buffer,
        scheduler::scheduler::FutureResult,
        QDesc,
    },
};
use ::std::{
    cell::RefCell,
    fmt,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
};

pub enum TcpOperation {
    Accept(FutureResult<AcceptFuture>),
    Connect(FutureResult<ConnectFuture>),
    Pop(FutureResult<PopFuture>),
    Push(FutureResult<PushFuture>),
}

impl From<AcceptFuture> for TcpOperation {
    fn from(f: AcceptFuture) -> Self {
        TcpOperation::Accept(FutureResult::new(f, None))
    }
}

impl From<ConnectFuture> for TcpOperation {
    fn from(f: ConnectFuture) -> Self {
        TcpOperation::Connect(FutureResult::new(f, None))
    }
}

impl From<PushFuture> for TcpOperation {
    fn from(f: PushFuture) -> Self {
        TcpOperation::Push(FutureResult::new(f, None))
    }
}

impl From<PopFuture> for TcpOperation {
    fn from(f: PopFuture) -> Self {
        TcpOperation::Pop(FutureResult::new(f, None))
    }
}

impl Future for TcpOperation {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        match self.get_mut() {
            TcpOperation::Accept(ref mut f) => Future::poll(Pin::new(f), ctx),
            TcpOperation::Connect(ref mut f) => Future::poll(Pin::new(f), ctx),
            TcpOperation::Push(ref mut f) => Future::poll(Pin::new(f), ctx),
            TcpOperation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx),
        }
    }
}

impl TcpOperation {
    pub fn expect_result(self) -> (QDesc, Option<QDesc>, OperationResult) {
        match self {
            // Connect operation.
            TcpOperation::Connect(FutureResult {
                future,
                done: Some(Ok(())),
            }) => (future.fd, None, OperationResult::Connect),
            TcpOperation::Connect(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.fd, None, OperationResult::Failed(e)),

            // Accept operation.
            TcpOperation::Accept(FutureResult {
                future,
                done: Some(Ok(new_qd)),
            }) => (future.qd, Some(future.new_qd), OperationResult::Accept(new_qd)),
            TcpOperation::Accept(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.qd, Some(future.new_qd), OperationResult::Failed(e)),

            // Push operation
            TcpOperation::Push(FutureResult {
                future,
                done: Some(Ok(())),
            }) => (future.fd, None, OperationResult::Push),
            TcpOperation::Push(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.fd, None, OperationResult::Failed(e)),

            // Pop Operation.
            TcpOperation::Pop(FutureResult {
                future,
                done: Some(Ok(bytes)),
            }) => (future.fd, None, OperationResult::Pop(None, bytes)),
            TcpOperation::Pop(FutureResult {
                future,
                done: Some(Err(e)),
            }) => (future.fd, None, OperationResult::Failed(e)),

            _ => panic!("Future not ready"),
        }
    }
}

pub struct ConnectFuture {
    pub fd: QDesc,
    pub inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for ConnectFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ConnectFuture({:?})", self.fd)
    }
}

impl Future for ConnectFuture {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        self_.inner.borrow_mut().poll_connect_finished(self_.fd, context)
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
    type Output = Result<QDesc, Fail>;

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
    pub fd: QDesc,
    pub err: Option<Fail>,
}

impl fmt::Debug for PushFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PushFuture({:?})", self.fd)
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
    pub fd: QDesc,
    pub inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for PopFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PopFuture({:?})", self.fd)
    }
}

impl Future for PopFuture {
    type Output = Result<Buffer, Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        let peer = TcpPeer {
            inner: self_.inner.clone(),
        };
        peer.poll_recv(self_.fd, ctx)
    }
}
