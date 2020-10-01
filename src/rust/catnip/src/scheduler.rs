use crate::collections::async_slab::AsyncSlab;
use std::rc::Rc;
use std::cell::RefCell;
use std::pin::Pin;
use std::future::Future;
use std::task::Context;
use crate::runtime::Runtime;
use crate::protocols::tcp::peer::{
    ConnectFuture,
    AcceptFuture,
    PushFuture,
    PopFuture,
};

enum ForegroundFuture<RT: Runtime> {
    Connect(ConnectFuture<RT>),
    Accept(AcceptFuture<RT>),
    Push(PushFuture<RT>),
    Pop(PopFuture<RT>),
}

enum ScheduledFuture<RT: Runtime> {
    // These are all stored inline to prevent hitting the allocator on insertion/removal.
    Foreground(ForegroundFuture<RT>),

    // These are expected to have long lifetimes and be large enough to justify another allocation.
    Background(Pin<Box<dyn Future<Output=()>>>),
}

pub struct Scheduler<RT: Runtime> {
    inner: Rc<RefCell<Inner<RT>>>,
}

struct Inner<RT: Runtime> {
    slab: AsyncSlab<ScheduledFuture<RT>>,
}

impl<RT: Runtime> Scheduler<RT> {
    // Do we need to have a "finished" queue here?
    // The layer above can...
    // 1) Poll a specific qtoken
    // 2) Drop a qtoken
    // 3) Wait on a particular qtoken
    // 4) Wait on many qtokens
    //
    // If we don't make progress on any of the wait methods, we need to then consider...
    // 1) Polling incoming packets
    // 2) Doing background work
    // 3) Advancing time
    fn poll(&self, ctx: &mut Context) {
    }
}
