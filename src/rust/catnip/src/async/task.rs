use crate::prelude::*;
use std::{
    any::Any,
    fmt::{self, Debug},
    marker::Unpin,
    ops::{Generator, GeneratorState},
    pin::Pin,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum TaskStatus {
    Completed(Result<Rc<Any>>),
    AsleepUntil(Instant),
}

impl<T> Into<Result<T>> for TaskStatus
where
    T: 'static + Clone + Debug,
{
    fn into(self) -> Result<T> {
        trace!("TaskStatus::into()");
        match self {
            TaskStatus::Completed(r) => match r {
                Ok(x) => Ok(x.downcast_ref::<T>().unwrap().clone()),
                Err(Fail::TryAgain {}) => panic!(
                    "coroutines are not allowed to return `Fail::TryAgain`"
                ),
                Err(e) => Err(e.clone()),
            },
            _ => Err(Fail::TryAgain {}),
        }
    }
}

#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct TaskId(u64);

impl From<u64> for TaskId {
    fn from(n: u64) -> TaskId {
        TaskId(n)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl fmt::Debug for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TaskId({})", self.0)
    }
}

pub struct Task<'a> {
    id: TaskId,
    status: TaskStatus,
    gen: Box<
        Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    >,
}

impl<'a> Task<'a> {
    pub fn new<G>(id: TaskId, gen: G, now: Instant) -> Task<'a>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        Task {
            id,
            // initialize the task with a status that will cause it to be
            // awakened immediately.
            status: TaskStatus::AsleepUntil(now),
            gen: Box::new(gen),
        }
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn status(&self) -> &TaskStatus {
        &self.status
    }

    pub fn resume(&mut self, now: Instant) -> bool {
        match &self.status {
            // if the task has already completed, do nothing with the
            // generator (we would panic).
            TaskStatus::Completed(_) => true,
            TaskStatus::AsleepUntil(when) => {
                if now < *when {
                    panic!("attempt to resume a sleeping task");
                } else {
                    match Pin::new(self.gen.as_mut()).resume() {
                        GeneratorState::Yielded(duration) => {
                            // if `yield None` is used, then we schedule
                            // something for the next tick.
                            // todo: ensure that a zero duration is not used.
                            let duration = duration
                                .unwrap_or_else(|| Duration::new(0, 0));
                            self.status =
                                TaskStatus::AsleepUntil(now + duration);
                            false
                        }
                        GeneratorState::Complete(result) => {
                            debug!(
                                "coroutine {} status is now completed.",
                                self.id
                            );
                            self.status = TaskStatus::Completed(result);
                            true
                        }
                    }
                }
            }
        }
    }
}
