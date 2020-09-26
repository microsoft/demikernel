use crate::protocols::tcp2::runtime::Runtime;
use crate::protocols::tcp2::peer::{
    AcceptFuture,
    ConnectFuture,
    PushFuture,
    PopFuture,
};
use slab::Slab;

enum Operation<RT: Runtime> {
    Accept(AcceptFuture<RT>),
    Connect(ConnectFuture<RT>),
    Push(PushFuture<RT>),
    Pop(PopFuture<RT>),
}

pub struct QTokenManager<RT: Runtime> {
    slab: Slab<Operation<RT>>,
}
