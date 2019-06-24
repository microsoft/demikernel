use super::Future;
use crate::prelude::*;
use std::{collections::VecDeque, fmt::Debug, time::Instant};

pub struct WhenAny<'a, T>
where
    T: Clone + Debug,
{
    queue: VecDeque<Future<'a, T>>,
}

impl<'a, T> WhenAny<'a, T>
where
    T: Clone + Debug + 'static,
{
    pub fn new() -> WhenAny<'a, T> {
        WhenAny {
            queue: VecDeque::new(),
        }
    }

    pub fn add_future(&mut self, fut: Future<'a, T>) {
        self.queue.push_back(fut);
    }

    pub fn poll(&mut self, now: Instant) -> Result<T> {
        if self.queue.is_empty() {
            return Err(Fail::TryAgain {});
        }

        let fut = self.queue.pop_front().unwrap();
        match fut.poll(now) {
            Ok(x) => Ok(x),
            Err(Fail::TryAgain {}) => {
                self.queue.push_back(fut);
                Err(Fail::TryAgain {})
            }
            Err(e) => Err(e),
        }
    }
}
