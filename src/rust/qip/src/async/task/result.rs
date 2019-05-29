use super::{
    super::{state::AsyncState, Future},
    TaskId, TaskStatus,
};
use crate::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Clone)]
pub struct TaskResult<'a, T> {
    r#async: Rc<RefCell<AsyncState<'a, T>>>,
    tid: TaskId,
}

impl<'a, T> TaskResult<'a, T> {
    pub fn new(
        r#async: Rc<RefCell<AsyncState<'a, T>>>,
        tid: TaskId,
    ) -> TaskResult<'a, T> {
        TaskResult { r#async, tid }
    }
}

impl<'a, T> Future<T> for TaskResult<'a, T>
where
    T: Sized + Copy,
{
    fn completed(&self) -> bool {
        let r#async = self.r#async.borrow();
        let task = r#async.get_task(self.tid);
        match task.status() {
            TaskStatus::Completed(_) => true,
            TaskStatus::AsleepUntil(_) => false,
        }
    }

    fn poll(&mut self, now: Instant) -> Result<T> {
        let mut r#async = self.r#async.borrow_mut();
        r#async.poll(now)?;
        let task = r#async.get_task(self.tid);
        match task.status() {
            TaskStatus::AsleepUntil(_) => Err(Fail::TryAgain {}),
            TaskStatus::Completed(r) => r.clone(),
        }
    }
}
