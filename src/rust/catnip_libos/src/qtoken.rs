use catnip::runtime::Runtime;
use bytes::Bytes;
use catnip::protocols::tcp::peer::SocketDescriptor;
use catnip::fail::Fail;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use hashbrown::HashMap;
use hashbrown::hash_map::Entry;
use catnip::protocols::tcp::peer::{
    ConnectFuture,
    AcceptFuture,
    PushFuture,
    PopFuture,
};

pub type QToken = u64;

pub enum UserOperation<RT: Runtime> {
    Connect(ConnectFuture<RT>),
    Accept(AcceptFuture<RT>),
    Push(PushFuture<RT>),
    Pop(PopFuture<RT>),
}

use std::fmt;

impl<RT: Runtime> fmt::Debug for UserOperation<RT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
	    UserOperation::Connect(ref fut) => write!(f, "Connect{:?}", fut),
	    UserOperation::Accept(ref fut) => write!(f, "Accept{:?}", fut),
	    UserOperation::Push(ref fut) => write!(f, "Push{:?}", fut),
	    UserOperation::Pop(ref fut) => write!(f, "Pop{:?}", fut),
	}
    }
}

impl <RT: Runtime> UserOperation<RT> {
    fn fd(&self) -> SocketDescriptor {
        match self {
            UserOperation::Connect(ref f) => f.fd,
            UserOperation::Accept(ref f) => f.fd,
            UserOperation::Push(ref f) => f.fd,
            UserOperation::Pop(ref f) => f.fd,
        }
    }
}

impl<RT: Runtime> Future for UserOperation<RT> {
    type Output = (SocketDescriptor, UserOperationResult);

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<(SocketDescriptor, UserOperationResult)> {
        let self_ = self.get_mut();
        let r = match self_ {
            UserOperation::Connect(ref mut f) => Future::poll(Pin::new(f), ctx).map(UserOperationResult::connect),
            UserOperation::Accept(ref mut f) => Future::poll(Pin::new(f), ctx).map(UserOperationResult::accept),
            UserOperation::Push(ref mut f) => Future::poll(Pin::new(f), ctx).map(UserOperationResult::push),
            UserOperation::Pop(ref mut f) => Future::poll(Pin::new(f), ctx).map(UserOperationResult::pop),
        };
        r.map(|r| (self_.fd(), r))
    }
}


#[derive(Debug)]
pub enum UserOperationResult {
    Connect,
    Accept(SocketDescriptor),
    Push,
    Pop(Bytes),
    Failed(Fail),
    InvalidToken,
}

impl UserOperationResult {
    fn connect(r: Result<SocketDescriptor, Fail>) -> Self {
        match r {
            Ok(..) => UserOperationResult::Connect,
            Err(e) => UserOperationResult::Failed(e),
        }
    }

    fn accept(r: Result<SocketDescriptor, Fail>) -> Self {
        match r {
            Ok(qd) => UserOperationResult::Accept(qd),
            Err(e) => UserOperationResult::Failed(e),
        }
    }

    fn push(r: Result<(), Fail>) -> Self {
        match r {
            Ok(()) => UserOperationResult::Push,
            Err(e) => UserOperationResult::Failed(e),
        }
    }

    fn pop(r: Result<Bytes, Fail>) -> Self {
        match r {
            Ok(r) => UserOperationResult::Pop(r),
            Err(e) => UserOperationResult::Failed(e),
        }
    }
}


pub struct QTokenManager<RT: Runtime> {
    next_token: QToken,
    qtokens: HashMap<QToken, UserOperation<RT>>,
}

impl<RT: Runtime> QTokenManager<RT> {
    pub fn new() -> Self {
        Self {
            next_token: 1,
            qtokens: HashMap::new(),
        }
    }

    pub fn insert(&mut self, operation: UserOperation<RT>) -> QToken {
        let token = self.next_token;
        self.next_token += 1;
        assert!(self.qtokens.insert(token, operation).is_none());
        token
    }

    pub fn remove(&mut self, token: QToken) -> bool {
        self.qtokens.remove(&token).is_some()
    }

    pub fn poll(&mut self, token: QToken, ctx: &mut Context) -> Poll<(SocketDescriptor, UserOperationResult)> {
        let mut entry = match self.qtokens.entry(token) {
            Entry::Occupied(e) => e,
            Entry::Vacant(..) => return Poll::Ready((0, UserOperationResult::InvalidToken)),
        };
        let r = Future::poll(Pin::new(entry.get_mut()), ctx);
        if r.is_ready() {
            entry.remove();
        }
        r
    }

}
