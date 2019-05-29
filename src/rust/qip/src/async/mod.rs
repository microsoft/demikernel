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
use task::{TaskId, TaskResult};

pub use future::Future;

pub struct Async<'a, T> {
    state: Rc<RefCell<AsyncState<'a, T>>>,
}

impl<'a, T> Async<'a, T>
where
    T: Copy,
{
    pub fn new(now: Instant) -> Self {
        Async {
            state: Rc::new(RefCell::new(AsyncState::new(now))),
        }
    }

    pub fn start_task<G>(&self, gen: G) -> TaskResult<'a, T>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<T>>
            + 'a
            + Unpin,
    {
        let mut state = self.state.borrow_mut();
        let tid = state.start_task(gen);
        TaskResult::new(self.state.clone(), tid)
    }

    pub fn drop_task(&mut self, tid: TaskId) {
        let mut state = self.state.borrow_mut();
        state.drop_task(tid)
    }
}
