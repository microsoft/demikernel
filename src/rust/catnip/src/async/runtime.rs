// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    coroutine::{Coroutine, CoroutineId, CoroutineStatus},
    future::Future,
    schedule::Schedule,
    traits::Async,
};
use crate::prelude::*;
use fxhash::{FxHashMap, FxHashSet};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt::Debug,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct AsyncRuntime<'a> {
    active_coroutines: Rc<RefCell<FxHashSet<CoroutineId>>>,
    inactive_coroutines: Rc<RefCell<FxHashMap<CoroutineId, Coroutine<'a>>>>,
    next_unused_id: Rc<Cell<u64>>,
    schedule: Rc<RefCell<Schedule>>,
}

impl<'a> AsyncRuntime<'a> {
    pub fn new(now: Instant) -> Self {
        AsyncRuntime {
            active_coroutines: Rc::new(RefCell::new(FxHashSet::default())),
            inactive_coroutines: Rc::new(RefCell::new(FxHashMap::default())),
            next_unused_id: Rc::new(Cell::new(0)),
            schedule: Rc::new(RefCell::new(Schedule::new(now))),
        }
    }

    pub fn clock(&self) -> Instant {
        self.schedule.borrow().clock()
    }

    pub fn start_coroutine<G, T>(&self, gen: G) -> Future<'a, T>
    where
        T: Any + Clone + Debug + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<dyn Any>>>
            + 'a
            + Unpin,
    {
        let cid = self.new_tid();
        let co = Coroutine::new(cid, gen, self.clock());
        self.schedule.borrow_mut().schedule(&co);
        self.inactive_coroutines.borrow_mut().insert(cid, co);
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

    fn poll_schedule(&self, now: Instant) -> Option<CoroutineId> {
        // we had to extract this into its own function to limit the scope
        // of the mutable borrow (it was causing borrowing deadlocks).
        self.schedule.borrow_mut().poll(now)
    }

    pub fn drop_coroutine(&self, cid: CoroutineId) -> Result<()> {
        trace!("AsyncRuntime::drop_coroutine({})", cid);
        // this function should not panic as it's called from `drop()`.
        self.schedule.try_borrow_mut()?.cancel(cid);
        self.inactive_coroutines.try_borrow_mut()?.remove(&cid);
        Ok(())
    }

    pub fn coroutine_status(&self, cid: CoroutineId) -> CoroutineStatus {
        trace!("AsyncRuntime::coroutine_status({})", cid);
        if self.active_coroutines.borrow().contains(&cid) {
            CoroutineStatus::Active
        } else {
            self.inactive_coroutines
                .borrow()
                .get(&cid)
                .unwrap()
                .status()
                .clone()
        }
    }
}

impl<'a> Async<CoroutineId> for AsyncRuntime<'a> {
    fn poll(&self, now: Instant) -> Option<Result<CoroutineId>> {
        trace!("AsyncRuntime::poll({:?})", now);
        if let Some(cid) = self.poll_schedule(now) {
            trace!("coroutine (cid = {}) is now active", cid);
            let mut coroutine = {
                let mut inactive_coroutines =
                    self.inactive_coroutines.borrow_mut();
                // coroutine has to be removed from `self.inactive_coroutines`
                // in order to work around a mutablility
                // deadlock when futures are used from within a
                // coroutine. we also don't anticipate a
                // reasonable situation where the schedule would give us an ID
                // that isn't in `self.inactive_coroutines`.
                let coroutine = inactive_coroutines.remove(&cid).unwrap();
                assert!(self.active_coroutines.borrow_mut().insert(cid));
                coroutine
            };

            if !coroutine.resume(now) {
                self.schedule.borrow_mut().schedule(&coroutine);
            }

            let cid = coroutine.id();
            trace!("coroutine {} yielded (`{:?}`)", cid, coroutine.status());
            assert!(self
                .inactive_coroutines
                .borrow_mut()
                .insert(cid, coroutine)
                .is_none());
            assert!(self.active_coroutines.borrow_mut().remove(&cid));
            Some(Ok(cid))
        } else {
            None
        }
    }
}
