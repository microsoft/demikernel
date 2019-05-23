use crate::prelude::*;
use std::rc::Rc;
use std::{
    marker::Unpin,
    ops::{Generator, GeneratorState},
    pin::Pin,
    time::{Duration, Instant},
};

pub enum Status<T> {
    Completed(Result<Rc<T>>),
    AsleepUntil(Instant),
}

impl<T> Into<Result<Rc<T>>> for Status<T> {
    fn into(self) -> Result<Rc<T>> {
        match self {
            Status::Completed(r) => match r {
                Err(Fail::TryAgain {}) => panic!(
                    "coroutines are not allowed to return `Fail::TryAgain`"
                ),
                _ => r,
            },
            _ => Err(Fail::TryAgain {}),
        }
    }
}

impl<T> Clone for Status<T> {
    // deriving `Clone` for this struct didn't appear to work, so we implement it ourselves.
    fn clone(&self) -> Self {
        match self {
            Status::Completed(r) => match r {
                Ok(t) => Status::Completed(Ok(t.clone())),
                Err(e) => Status::Completed(Err(e.clone())),
            },
            Status::AsleepUntil(t) => Status::AsleepUntil(*t),
        }
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Copy)]
pub struct Id(u64);

impl From<u64> for Id {
    fn from(n: u64) -> Id {
        Id(n)
    }
}

pub struct Task<'a, T> {
    id: Id,
    status: Status<T>,
    gen: Box<
        Generator<Yield = Option<Duration>, Return = Result<Rc<T>>>
            + 'a
            + Unpin,
    >,
}

impl<'a, T> Task<'a, T> {
    pub fn new<G>(id: Id, gen: G, now: Instant) -> Task<'a, T>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<T>>>
            + 'a
            + Unpin,
    {
        Task {
            id,
            // initialize the task with a status that will cause it to be
            // awakened immediately.
            status: Status::AsleepUntil(now),
            gen: Box::new(gen),
        }
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn status(&self) -> &Status<T> {
        &self.status
    }

    pub fn resume(&mut self, now: Instant) -> Result<Rc<T>> {
        match &self.status {
            // if the task has already completed, do nothing with the
            // generator (we would panic).
            Status::Completed(_) => (),
            Status::AsleepUntil(when) => {
                if now < *when {
                    panic!("attempt to awaken a sleeping task");
                } else {
                    match Pin::new(self.gen.as_mut()).resume() {
                        GeneratorState::Yielded(duration) => {
                            let duration = duration
                                .unwrap_or_else(|| Duration::new(0, 0));
                            self.status = Status::AsleepUntil(now + duration);
                        }
                        GeneratorState::Complete(result) => {
                            self.status = Status::Completed(result);
                        }
                    }
                }
            }
        }

        self.status.clone().into()
    }
}
