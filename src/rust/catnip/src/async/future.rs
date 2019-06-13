use super::{
    task::{TaskId, TaskStatus},
    Async,
};
use crate::prelude::*;
use std::{fmt::Debug, time::Instant};

#[derive(Clone)]
pub enum Future<'a, T>
where
    T: Clone + Debug,
{
    Const(Result<T>),
    TaskResult { r#async: Async<'a>, tid: TaskId },
}

impl<'a, T> Future<'a, T>
where
    T: Clone + Debug + 'static,
{
    pub fn r#const(value: T) -> Future<'a, T> {
        Future::Const(Ok(value))
    }

    pub fn task_result(r#async: Async<'a>, tid: TaskId) -> Future<'a, T> {
        Future::TaskResult { r#async, tid }
    }

    pub fn completed(&self) -> bool {
        match self {
            Future::Const(_) => true,
            Future::TaskResult { r#async, tid } => {
                match r#async.task_status(*tid) {
                    TaskStatus::Completed(_) => true,
                    TaskStatus::AsleepUntil(_) => false,
                }
            }
        }
    }

    pub fn poll(&self, now: Instant) -> Result<T>
    where
        T: Debug,
    {
        trace!("entering `Future::poll()`");
        match self {
            Future::Const(v) => v.clone(),
            Future::TaskResult { r#async, tid } => {
                // todo: should this return an error if poll() fails?
                let _ = r#async.poll(now);
                let status = r#async.task_status(*tid);
                debug!("status of coroutine {} is `{:?}`.", tid, status);
                status.into()
            }
        }
    }
}

impl<'a, T> Drop for Future<'a, T>
where
    T: Clone + Debug,
{
    fn drop(&mut self) {
        match self {
            Future::Const(_) => (),
            Future::TaskResult { r#async, tid } => {
                r#async.drop_task(*tid);
            }
        }
    }
}
