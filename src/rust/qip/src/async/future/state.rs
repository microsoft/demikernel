use super::super::{
    state::AsyncState,
    task::{TaskId, TaskStatus},
};
use crate::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Clone)]
pub enum FutureState<'a, T>
where
    T: Clone,
{
    Const(Result<T>),
    TaskResult {
        r#async: Rc<RefCell<AsyncState<'a, T>>>,
        tid: TaskId,
    },
}

impl<'a, T> FutureState<'a, T>
where
    T: Clone,
{
    pub fn completed(&self) -> bool {
        match self {
            FutureState::Const(_) => true,
            FutureState::TaskResult { r#async, tid } => {
                let r#async = r#async.borrow();
                match r#async.task_status(*tid) {
                    TaskStatus::Completed(_) => true,
                    TaskStatus::AsleepUntil(_) => false,
                }
            }
        }
    }

    pub fn poll(&mut self, now: Instant) -> Result<T> {
        eprintln!("# FutureState::poll()");
        match self {
            FutureState::Const(v) => v.clone(),
            FutureState::TaskResult { r#async, tid } => {
                let mut r#async = r#async.borrow_mut();
                r#async.poll(now)?;
                let status = r#async.task_status(*tid);
                match status {
                    TaskStatus::AsleepUntil(_) => Err(Fail::TryAgain {}),
                    TaskStatus::Completed(r) => r.clone(),
                }
            }
        }
    }
}

impl<'a, T> Drop for FutureState<'a, T>
where
    T: Clone,
{
    fn drop(&mut self) {
        match self {
            FutureState::Const(_) => (),
            FutureState::TaskResult { r#async, tid } => {
                let mut r#async = r#async.borrow_mut();
                r#async.drop_task(*tid);
            }
        }
    }
}
