use super::peer::{
    Inner,
    Peer,
};
use crate::{
    fail::Fail,
    file_table::FileDescriptor,
    operations::{
        OperationResult,
        ResultFuture,
    },
    runtime::Runtime,
};
use std::{
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

pub enum TcpOperation<RT: Runtime> {
    Accept(ResultFuture<AcceptFuture<RT>>),
    Connect(ResultFuture<ConnectFuture<RT>>),
    Pop(ResultFuture<PopFuture<RT>>),
    Push(ResultFuture<PushFuture<RT>>),
}

impl<RT: Runtime> From<AcceptFuture<RT>> for TcpOperation<RT> {
    fn from(f: AcceptFuture<RT>) -> Self {
        TcpOperation::Accept(ResultFuture::new(f))
    }
}

impl<RT: Runtime> From<ConnectFuture<RT>> for TcpOperation<RT> {
    fn from(f: ConnectFuture<RT>) -> Self {
        TcpOperation::Connect(ResultFuture::new(f))
    }
}

impl<RT: Runtime> From<PushFuture<RT>> for TcpOperation<RT> {
    fn from(f: PushFuture<RT>) -> Self {
        TcpOperation::Push(ResultFuture::new(f))
    }
}

impl<RT: Runtime> From<PopFuture<RT>> for TcpOperation<RT> {
    fn from(f: PopFuture<RT>) -> Self {
        TcpOperation::Pop(ResultFuture::new(f))
    }
}

impl<RT: Runtime> Future for TcpOperation<RT> {
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

impl<RT: Runtime> TcpOperation<RT> {
    pub fn expect_result(self) -> (FileDescriptor, OperationResult<RT>) {
        use TcpOperation::*;

        match self {
            Connect(ResultFuture {
                future,
                done: Some(Ok(())),
            }) => (future.fd, OperationResult::Connect),
            Connect(ResultFuture {
                future,
                done: Some(Err(e)),
            }) => (future.fd, OperationResult::Failed(e)),

            Accept(ResultFuture {
                future,
                done: Some(Ok(fd)),
            }) => (future.fd, OperationResult::Accept(fd)),
            Accept(ResultFuture {
                future,
                done: Some(Err(e)),
            }) => (future.fd, OperationResult::Failed(e)),

            Push(ResultFuture {
                future,
                done: Some(Ok(())),
            }) => (future.fd, OperationResult::Push),
            Push(ResultFuture {
                future,
                done: Some(Err(e)),
            }) => (future.fd, OperationResult::Failed(e)),

            Pop(ResultFuture {
                future,
                done: Some(Ok(bytes)),
            }) => (future.fd, OperationResult::Pop(None, bytes)),
            Pop(ResultFuture {
                future,
                done: Some(Err(e)),
            }) => (future.fd, OperationResult::Failed(e)),

            _ => panic!("Future not ready"),
        }
    }
}

pub enum ConnectFutureState {
    Failed(Fail),
    InProgress,
}

pub struct ConnectFuture<RT: Runtime> {
    pub fd: FileDescriptor,
    pub state: ConnectFutureState,
    pub inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> fmt::Debug for ConnectFuture<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ConnectFuture({})", self.fd)
    }
}

impl<RT: Runtime> Future for ConnectFuture<RT> {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        match self_.state {
            ConnectFutureState::Failed(ref e) => Poll::Ready(Err(e.clone())),
            ConnectFutureState::InProgress => self_
                .inner
                .borrow_mut()
                .poll_connect_finished(self_.fd, context),
        }
    }
}

pub struct AcceptFuture<RT: Runtime> {
    pub fd: FileDescriptor,
    pub inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> fmt::Debug for AcceptFuture<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AcceptFuture({})", self.fd)
    }
}

impl<RT: Runtime> Future for AcceptFuture<RT> {
    type Output = Result<FileDescriptor, Fail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        let peer = Peer {
            inner: self_.inner.clone(),
        };
        peer.poll_accept(self_.fd, context)
    }
}

pub struct PushFuture<RT: Runtime> {
    pub fd: FileDescriptor,
    pub err: Option<Fail>,
    pub _marker: std::marker::PhantomData<RT>,
}

impl<RT: Runtime> fmt::Debug for PushFuture<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PushFuture({})", self.fd)
    }
}

impl<RT: Runtime> Future for PushFuture<RT> {
    type Output = Result<(), Fail>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context) -> Poll<Self::Output> {
        match self.get_mut().err.take() {
            None => Poll::Ready(Ok(())),
            Some(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct PopFuture<RT: Runtime> {
    pub fd: FileDescriptor,
    pub inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> fmt::Debug for PopFuture<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PopFuture({})", self.fd)
    }
}

impl<RT: Runtime> Future for PopFuture<RT> {
    type Output = Result<RT::Buf, Fail>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        let peer = Peer {
            inner: self_.inner.clone(),
        };
        peer.poll_recv(self_.fd, ctx)
    }
}
