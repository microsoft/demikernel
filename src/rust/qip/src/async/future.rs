use super::{
    state::AsyncState,
    task::{TaskId, TaskStatus},
};
use crate::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Instant};

pub struct Future<'a, T> {
    r#async: Rc<RefCell<AsyncState<'a, T>>>,
    tid: TaskId,
}

impl<'a, T> Future<'a, T> {
    pub fn new(
        r#async: Rc<RefCell<AsyncState<'a, T>>>,
        tid: TaskId,
    ) -> Future<'a, T> {
        Future { r#async, tid }
    }

    pub fn tid(&self) -> TaskId {
        self.tid
    }

    pub fn status(&mut self) -> TaskStatus<T> {
        let r#async = self.r#async.borrow();
        let task = r#async.get_task(self.tid);
        task.status().clone()
    }

    pub fn poll(&mut self, now: Instant) -> Result<Rc<T>> {
        {
            let mut r#async = self.r#async.borrow_mut();
            r#async.poll(now)?;
        }

        let status = self.status();
        match status {
            TaskStatus::AsleepUntil(_) => Err(Fail::TryAgain {}),
            TaskStatus::Completed(r) => r.clone(),
        }
    }
}
