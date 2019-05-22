use crate::prelude::*;
use std::{
    marker::Unpin,
    ops::{Generator, GeneratorState},
    pin::Pin,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub enum Status {
    Completed(Result<()>),
    AsleepUntil(Instant),
}

#[derive(Eq, PartialEq, Hash, Clone, Copy)]
pub struct Id(u64);

impl From<u64> for Id {
    fn from(n: u64) -> Id {
        Id(n)
    }
}

pub struct Task<'a> {
    id: Id,
    status: Status,
    gen: Box<
        Generator<Yield = Option<Duration>, Return = Result<()>> + 'a + Unpin,
    >,
}

impl<'a> Task<'a> {
    pub fn new<G>(id: Id, gen: G, now: Instant) -> Task<'a>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<()>>
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

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn resume(&mut self, now: Instant) -> Result<bool> {
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
                            let duration =
                                duration.unwrap_or(Duration::new(0, 0));
                            self.status = Status::AsleepUntil(now + duration);
                            return Ok(false);
                        }
                        GeneratorState::Complete(result) => {
                            self.status = Status::Completed(result);
                        }
                    }
                }
            }
        }

        // at this point, we expect `self.status` to be `Completed(_)`.
        match &self.status {
            Status::Completed(result) => match result {
                Ok(()) => Ok(true),
                Err(e) => Err(e.clone()),
            },
            _ => {
                panic!("expected task status to be `Completed(_)`");
            }
        }
    }
}
