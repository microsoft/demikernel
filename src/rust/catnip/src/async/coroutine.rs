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
pub enum CoroutineStatus {
    Completed(Result<Rc<Any>>),
    AsleepUntil(Instant),
}

impl<T> Into<Option<Result<T>>> for CoroutineStatus
where
    T: 'static + Clone + Debug,
{
    fn into(self) -> Option<Result<T>> {
        trace!("CoroutineStatus::into({:?})", self);
        if let CoroutineStatus::Completed(r) = self {
            Some(r.map(|x| x.downcast_ref::<T>().unwrap().clone()))
        } else {
            None
        }
    }
}

#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct CoroutineId(u64);

impl From<u64> for CoroutineId {
    fn from(n: u64) -> CoroutineId {
        CoroutineId(n)
    }
}

impl fmt::Display for CoroutineId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl fmt::Debug for CoroutineId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CoroutineId({})", self.0)
    }
}

pub struct Coroutine<'a> {
    id: CoroutineId,
    status: CoroutineStatus,
    gen: Box<
        Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    >,
}

impl<'a> Coroutine<'a> {
    pub fn new<G>(id: CoroutineId, gen: G, now: Instant) -> Coroutine<'a>
    where
        G: Generator<Yield = Option<Duration>, Return = Result<Rc<Any>>>
            + 'a
            + Unpin,
    {
        Coroutine {
            id,
            // initialize the coroutine with a status that will cause it to be
            // awakened immediately.
            status: CoroutineStatus::AsleepUntil(now),
            gen: Box::new(gen),
        }
    }

    pub fn id(&self) -> CoroutineId {
        self.id
    }

    pub fn status(&self) -> &CoroutineStatus {
        &self.status
    }

    pub fn resume(&mut self, now: Instant) -> bool {
        match &self.status {
            // if the coroutine has already completed, do nothing with the
            // generator (we would panic).
            CoroutineStatus::Completed(_) => true,
            CoroutineStatus::AsleepUntil(when) => {
                if now < *when {
                    panic!("attempt to resume a sleeping coroutine");
                } else {
                    match Pin::new(self.gen.as_mut()).resume() {
                        GeneratorState::Yielded(duration) => {
                            // if `yield None` is used, then we schedule
                            // something for the next tick.
                            // todo: ensure that a zero duration is not used.
                            let duration = duration
                                .unwrap_or_else(|| Duration::new(0, 0));
                            self.status =
                                CoroutineStatus::AsleepUntil(now + duration);
                            false
                        }
                        GeneratorState::Complete(result) => {
                            debug!(
                                "coroutine {} status is now completed.",
                                self.id
                            );
                            self.status = CoroutineStatus::Completed(result);
                            true
                        }
                    }
                }
            }
        }
    }
}
