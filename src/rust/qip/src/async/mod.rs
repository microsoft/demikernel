mod future;
mod schedule;
mod state;
mod task;

use crate::prelude::*;
use state::AsyncState;
use std::{
    any::Any,
    cell::RefCell,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};
use task::{TaskId, TaskStatus};

pub use future::Future;

#[derive(Clone)]
pub struct Async<'a> {
    state: Rc<RefCell<AsyncState<'a>>>,
}

impl<'a> Async<'a> {
    pub fn new(now: Instant) -> Self {
        Async {
            state: Rc::new(RefCell::new(AsyncState::new(now))),
        }
    }

    pub fn start_task<G, T>(&self, gen: G) -> Future<'a, T>
    where
        T: Any + Clone + 'static,
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        let tid = self.state.borrow_mut().start_task(gen);
        Future::task_result(self.clone(), tid)
    }

    pub fn drop_task(&self, tid: TaskId) {
        self.state.borrow_mut().drop_task(tid)
    }

    pub fn task_status(&self, tid: TaskId) -> TaskStatus {
        self.state.borrow().task_status(tid).clone()
    }

    pub fn service(&self, now: Instant) {
        let _ = self.state.borrow_mut().poll(now);
    }
}
