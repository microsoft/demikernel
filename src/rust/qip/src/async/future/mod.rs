mod state;

use super::{state::AsyncState, task::TaskId};
use crate::prelude::*;
use state::FutureState as State;
use std::{cell::RefCell, rc::Rc, time::Instant};

pub struct Future<'a, T>
where
    T: Clone,
{
    state: Rc<RefCell<State<'a, T>>>,
}

impl<'a, T> Future<'a, T>
where
    T: Clone,
{
    pub fn r#const(t: T) -> Future<'a, T> {
        Future {
            state: Rc::new(RefCell::new(State::Const(Ok(t)))),
        }
    }

    pub fn task_result(
        r#async: Rc<RefCell<AsyncState<'a, T>>>,
        tid: TaskId,
    ) -> Future<'a, T> {
        Future {
            state: Rc::new(RefCell::new(State::TaskResult { r#async, tid })),
        }
    }

    pub fn completed(&self) -> bool {
        let state = self.state.borrow();
        state.completed()
    }

    pub fn poll(&self, now: Instant) -> Result<T> {
        let mut state = self.state.borrow_mut();
        state.poll(now)
    }
}
