use crate::prelude::*;
use std::{
    fmt,
    marker::Unpin,
    ops::{Generator, GeneratorState},
    pin::Pin,
    time::{Duration, Instant},
};

pub enum TaskStatus<T> {
    Completed(Result<T>),
    AsleepUntil(Instant),
}

impl<T> Into<Result<T>> for TaskStatus<T> {
    fn into(self) -> Result<T> {
        match self {
            TaskStatus::Completed(r) => match r {
                Err(Fail::TryAgain {}) => panic!(
                    "coroutines are not allowed to return `Fail::TryAgain`"
                ),
                _ => r,
            },
            _ => Err(Fail::TryAgain {}),
        }
    }
}

impl<T> Clone for TaskStatus<T>
where
    T: Clone,
{
    // deriving `Clone` for this struct didn't appear to work, so we implement
    // it ourselves.
    fn clone(&self) -> Self {
        match self {
            TaskStatus::Completed(r) => match r {
                Ok(t) => TaskStatus::Completed(Ok(t.clone())),
                Err(e) => TaskStatus::Completed(Err(e.clone())),
            },
            TaskStatus::AsleepUntil(t) => TaskStatus::AsleepUntil(*t),
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
        self.0.fmt(f)
    }
}

impl fmt::Debug for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MacAddress({})", self.0)
    }
}

pub struct Task<'a, T> {
    id: TaskId,
    status: TaskStatus<T>,
    gen: Box<
        Generator<Yield = Option<Duration>, Return = Result<T>> + 'a + Unpin,
    >,
}

impl<'a, T> Task<'a, T>
where
    T: Clone,
{
    pub fn new<G>(id: TaskId, gen: G, now: Instant) -> Task<'a, T>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<T>>
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

    pub fn status(&self) -> &TaskStatus<T> {
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
                            let duration = duration
                                .unwrap_or_else(|| Duration::new(0, 0));
                            self.status =
                                TaskStatus::AsleepUntil(now + duration);
                            false
                        }
                        GeneratorState::Complete(result) => {
                            self.status = TaskStatus::Completed(result);
                            true
                        }
                    }
                }
            }
        }
    }
}
