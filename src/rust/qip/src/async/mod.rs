mod future;
mod schedule;
mod state;
mod task;

use crate::prelude::*;
use state::AsyncState;
use std::{
    cell::RefCell,
    ops::Generator,
    rc::Rc,
    time::{Duration, Instant},
};
use task::{TaskId, TaskStatus};

pub use future::Future;

#[derive(Clone)]
pub struct Async<'a, T> {
    state: Rc<RefCell<AsyncState<'a, T>>>,
}

impl<'a, T> Async<'a, T>
where
    T: Clone,
{
    pub fn new(now: Instant) -> Self {
        Async {
            state: Rc::new(RefCell::new(AsyncState::new(now))),
        }
    }

    pub fn start_task<G>(&self, gen: G) -> Future<'a, T>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<T>>
            + 'a
            + Unpin,
    {
        let mut state = self.state.borrow_mut();
        let tid = state.start_task(gen);
        Future::task_result(self.clone(), tid)
    }

    pub fn drop_task(&self, tid: TaskId) {
        let mut state = self.state.borrow_mut();
        state.drop_task(tid)
    }

    pub fn task_status(&self, tid: TaskId) -> TaskStatus<T> {
        let state = self.state.borrow();
        state.task_status(tid).clone()
    }

    pub fn service(&self, now: Instant) {
        let mut state = self.state.borrow_mut();
        let _ = state.poll(now);
    }
}
