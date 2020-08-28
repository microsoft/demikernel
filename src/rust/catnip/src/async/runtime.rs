// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    coroutine::CoroutineId,
    schedule::Schedule,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

#[derive(Clone)]
pub struct AsyncRuntime {
    next_unused_id: Rc<Cell<u64>>,
    schedule: Rc<RefCell<Schedule>>,
}

impl AsyncRuntime {
    pub fn new(now: Instant) -> Self {
        AsyncRuntime {
            next_unused_id: Rc::new(Cell::new(0)),
            schedule: Rc::new(RefCell::new(Schedule::new(now))),
        }
    }

    pub fn clock(&self) -> Instant {
        self.schedule.borrow().clock()
    }

    fn poll_schedule(&self, now: Instant) -> Option<CoroutineId> {
        // we had to extract this into its own function to limit the scope
        // of the mutable borrow (it was causing borrowing deadlocks).
        self.schedule.borrow_mut().poll(now)
   }
}

// impl Async<CoroutineId> for AsyncRuntime {
//     fn poll(&self, now: Instant) -> Option<Result<CoroutineId>> {
//         trace!("AsyncRuntime::poll({:?})", now);
//         match self.poll_schedule(now) {
//             Some(cid) if !self.inactive_coroutines.borrow().contains_key(&cid) =>
//             {
//                 // The coroutine returned by the schedule has been cancelled.
//                 // Just try again.
//                 self.poll(now)
//             }
//             Some(cid) => {
//                 trace!("coroutine (cid = {}) is now active", cid);
//                 let mut coroutine = {
//                     let mut inactive_coroutines =
//                         self.inactive_coroutines.borrow_mut();
//                     // coroutine has to be removed from
//                     // `self.inactive_coroutines`
//                     // in order to work around a mutablility
//                     // deadlock when futures are used from within a
//                     // coroutine. we also don't anticipate a
//                     // reasonable situation where the schedule would give us an
//                     // ID that isn't in
//                     // `self.inactive_coroutines`.
//                     inactive_coroutines.remove(&cid).unwrap()
//                 };

//                 if !coroutine.resume(now) {
//                     self.schedule.borrow_mut().schedule(&coroutine);
//                 }

//                 let cid = coroutine.id();
//                 trace!(
//                     "coroutine {} yielded (`{:?}`)",
//                     cid,
//                     coroutine.status()
//                 );
//                 assert!(self
//                     .inactive_coroutines
//                     .borrow_mut()
//                     .insert(cid, coroutine)
//                     .is_none());
//                 Some(Ok(cid))
//             }
//             None => None,
//         }
//     }
// }
