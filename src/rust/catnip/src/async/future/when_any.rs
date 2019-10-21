// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{super::traits::Async, Future};
use crate::prelude::*;
use std::{
    cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc, time::Instant,
};

pub struct WhenAny<'a, T>
where
    T: Clone + Debug,
{
    queue: Rc<RefCell<VecDeque<Future<'a, T>>>>,
}

impl<'a, T> WhenAny<'a, T>
where
    T: Clone + Debug + 'static,
{
    pub fn new() -> WhenAny<'a, T> {
        WhenAny {
            queue: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    pub fn add(&mut self, fut: Future<'a, T>) {
        let mut queue = self.queue.borrow_mut();
        queue.push_back(fut);
    }
}

impl<'a, T> Default for WhenAny<'a, T>
where
    T: Clone + Debug + 'static,
{
    fn default() -> Self {
        WhenAny::new()
    }
}

impl<'a, T> Async<T> for WhenAny<'a, T>
where
    T: Clone + Debug + 'static,
{
    fn poll(&self, now: Instant) -> Option<Result<T>> {
        let mut queue = self.queue.borrow_mut();
        if queue.is_empty() {
            return None;
        }

        let fut = queue.pop_front().unwrap();
        let result = fut.poll(now);
        if result.is_none() {
            queue.push_back(fut);
        }

        result
    }
}
