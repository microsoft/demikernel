use super::{
    state::AsyncState,
    task::{TaskId, TaskStatus},
};
use crate::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Clone)]
pub enum Future<'a, T> {
    Const(Result<T>),
    TaskResult {
        r#async: Rc<RefCell<AsyncState<'a, T>>>,
        tid: TaskId,
    },
}

impl<'a, T> Future<'a, T>
where
    T: Copy,
{
    pub fn completed(&self) -> bool {
        match self {
            Future::Const(_) => true,
            Future::TaskResult { r#async, tid } => {
                let r#async = r#async.borrow();
                let task = r#async.get_task(*tid);
                match task.status() {
                    TaskStatus::Completed(_) => true,
                    TaskStatus::AsleepUntil(_) => false,
                }
            }
        }
    }

    pub fn poll(&mut self, now: Instant) -> Result<T> {
        match self {
            Future::Const(v) => v.clone(),
            Future::TaskResult { r#async, tid } => {
                let mut r#async = r#async.borrow_mut();
                r#async.poll(now)?;
                let task = r#async.get_task(*tid);
                match task.status() {
                    TaskStatus::AsleepUntil(_) => Err(Fail::TryAgain {}),
                    TaskStatus::Completed(r) => r.clone(),
                }
            }
        }
    }
}
