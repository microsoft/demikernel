mod coroutine;
mod future;
mod schedule;

use crate::prelude::*;
use coroutine::{Coroutine, CoroutineId, CoroutineStatus};
use schedule::Schedule;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::Debug,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};

pub use future::{Future, WhenAny};

#[derive(Clone)]
pub struct Async<'a> {
    next_unused_id: Rc<Cell<u64>>,
    coroutines: Rc<RefCell<HashMap<CoroutineId, Coroutine<'a>>>>,
    schedule: Rc<RefCell<Schedule>>,
}

impl<'a> Async<'a> {
    pub fn new(now: Instant) -> Self {
        Async {
            next_unused_id: Rc::new(Cell::new(0)),
            coroutines: Rc::new(RefCell::new(HashMap::new())),
            schedule: Rc::new(RefCell::new(Schedule::new(now))),
        }
    }

    pub fn clock(&self) -> Instant {
        self.schedule.borrow().clock()
    }

    pub fn start_coroutine<G, T>(&self, gen: G) -> Future<'a, T>
    where
        T: Any + Clone + Debug + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        let cid = self.new_tid();
        let co = Coroutine::new(cid, gen, self.clock());
        self.schedule.borrow_mut().schedule(&co);
        self.coroutines.borrow_mut().insert(cid, co);
        let fut = Future::coroutine_result(self.clone(), cid);
        let _ = fut.poll(self.clock());
        fut
    }

    fn new_tid(&self) -> CoroutineId {
        let n = self.next_unused_id.get();
        let cid = CoroutineId::from(n);
        // todo: we should deal with overflow.
        self.next_unused_id.set(n + 1);
        cid
    }

    pub fn poll_schedule(&self, now: Instant) -> Option<CoroutineId> {
        // we had to extract this into its own function to limit the scope
        // of the mutable borrow (it was causing borrowing deadlocks).
        self.schedule.borrow_mut().poll(now)
    }

    pub fn poll(&self, now: Instant) -> Result<CoroutineId> {
        trace!("Async::poll({:?})", now);
        if let Some(cid) = self.poll_schedule(now) {
            debug!("coroutine (cid = {:?}) is up", cid);
            let mut coroutine = {
                let mut coroutines = self.coroutines.borrow_mut();
                // coroutine has to be removed from coroutines in order to work
                // around a mutablility deadlock when futures
                // are used from within a coroutine. we also
                // don't anticipate a reasonable situation
                // where the schedule would give us an ID that isn't in
                // `self.coroutines`.
                coroutines.remove(&cid).unwrap()
            };

            if !coroutine.resume(now) {
                self.schedule.borrow_mut().schedule(&coroutine);
            }

            let cid = coroutine.id();
            debug!(
                "coroutine {} successfully resumed; status is now `{:?}`",
                cid,
                coroutine.status()
            );
            let mut coroutines = self.coroutines.borrow_mut();
            assert!(coroutines.insert(cid, coroutine).is_none());
            Ok(cid)
        } else {
            Err(Fail::TryAgain {})
        }
    }

    pub fn drop_coroutine(&self, cid: CoroutineId) -> Result<()> {
        // this function should not panic as it's called from `drop()`.
        self.schedule.try_borrow_mut()?.cancel(cid);
        self.coroutines.try_borrow_mut()?.remove(&cid);
        Ok(())
    }

    pub fn coroutine_status(&self, cid: CoroutineId) -> CoroutineStatus {
        self.coroutines.borrow().get(&cid).unwrap().status().clone()
    }
}
