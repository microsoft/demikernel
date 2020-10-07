use crate::runtime::Runtime;
use super::peer::{
    SocketDescriptor,
    Inner,
    Peer,
};
use crate::fail::Fail;
use bytes::Bytes;
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

pub enum ResultFuture<F: Future> {
    Pending(F),
    Done(F::Output),
}

impl<F: Future + Unpin> Future for ResultFuture<F>
    where F::Output: Unpin
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let self_ = self.get_mut();
        match self_ {
            ResultFuture::Pending(ref mut f) => {
                let result = match Future::poll(Pin::new(f), ctx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(r) => r,
                };
                *self_ = ResultFuture::Done(result);
                Poll::Ready(())
            },
            ResultFuture::Done(..) => panic!("Polled after completion"),
        }
    }
}

pub enum TcpOperation<RT: Runtime> {
    Accept(ResultFuture<AcceptFuture<RT>>),
    Connect(ResultFuture<ConnectFuture<RT>>),
    Pop(ResultFuture<PopFuture<RT>>),
    Push(ResultFuture<PushFuture<RT>>),
}

impl<RT: Runtime> From<AcceptFuture<RT>> for TcpOperation<RT> {
    fn from(f: AcceptFuture<RT>) -> Self {
        TcpOperation::Accept(ResultFuture::Pending(f))
    }
}

impl<RT: Runtime> From<ConnectFuture<RT>> for TcpOperation<RT> {
    fn from(f: ConnectFuture<RT>) -> Self {
        TcpOperation::Connect(ResultFuture::Pending(f))
    }
}

impl<RT: Runtime> From<PushFuture<RT>> for TcpOperation<RT> {
    fn from(f: PushFuture<RT>) -> Self {
        TcpOperation::Push(ResultFuture::Pending(f))
    }
}

impl<RT: Runtime> From<PopFuture<RT>> for TcpOperation<RT> {
    fn from(f: PopFuture<RT>) -> Self {
        TcpOperation::Pop(ResultFuture::Pending(f))
    }
}

impl<RT: Runtime> Future for TcpOperation<RT> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        match self.get_mut() {
            TcpOperation::Accept(ref mut f) => Future::poll(Pin::new(f), ctx),
            _ => todo!(),
        }
    }
}

pub enum ConnectFutureState {
    Failed(Fail),
    InProgress,
}

pub struct ConnectFuture<RT: Runtime> {
    pub fd: SocketDescriptor,
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
    pub fd: SocketDescriptor,
    pub inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> fmt::Debug for AcceptFuture<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AcceptFuture({})", self.fd)
    }
}

impl<RT: Runtime> Future for AcceptFuture<RT> {
    type Output = Result<SocketDescriptor, Fail>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        let peer = Peer {
            inner: self_.inner.clone(),
        };
        match peer.accept(self_.fd) {
            Ok(Some(fd)) => Poll::Ready(Ok(fd)),
            Ok(None) => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct PushFuture<RT: Runtime> {
    pub fd: SocketDescriptor,
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
    pub fd: SocketDescriptor,
    pub inner: Rc<RefCell<Inner<RT>>>,
}

impl<RT: Runtime> fmt::Debug for PopFuture<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PopFuture({})", self.fd)
    }
}

impl<RT: Runtime> Future for PopFuture<RT> {
    type Output = Result<Bytes, Fail>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context) -> Poll<Self::Output> {
        let self_ = self.get_mut();
        let peer = Peer {
            inner: self_.inner.clone(),
        };
        match peer.recv(self_.fd) {
            Ok(Some(bytes)) => Poll::Ready(Ok(bytes)),
            Ok(None) => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
