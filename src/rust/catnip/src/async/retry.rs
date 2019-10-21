// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::{
    ops::{Generator, GeneratorState},
    pin::Pin,
    time::Duration,
};

pub struct Retry<'a> {
    completed: bool,
    gen: Box<dyn Generator<Yield = Duration, Return = ()> + 'a + Unpin>,
}

impl<'a> Retry<'a> {
    pub fn new<G>(gen: G) -> Retry<'a>
    where
        G: Generator<Yield = Duration, Return = ()> + 'a + Unpin,
    {
        Retry {
            completed: false,
            gen: Box::new(gen),
        }
    }

    pub fn none(timeout: Duration) -> Retry<'a> {
        Retry::new(move || {
            yield timeout;
        })
    }

    pub fn periodic(timeout: Duration, count: usize) -> Retry<'a> {
        assert!(timeout > Duration::new(0, 0));
        assert!(count > 0);
        Retry::new(move || {
            for _ in 0..count {
                yield timeout;
            }
        })
    }

    pub fn exponential(
        start: Duration,
        factor: u32,
        count: usize,
    ) -> Retry<'a> {
        assert!(factor > 1);
        assert!(count > 0);
        Retry::new(move || {
            let mut timeout = start;
            for _ in 0..count {
                yield timeout;
                timeout *= factor;
            }
        })
    }

    pub fn binary_exponential(start: Duration, count: usize) -> Retry<'a> {
        Retry::exponential(start, 2, count)
    }
}

impl<'a> Iterator for Retry<'a> {
    type Item = Duration;

    fn next(&mut self) -> Option<Self::Item> {
        if self.completed {
            return None;
        }

        match Pin::new(self.gen.as_mut()).resume() {
            GeneratorState::Yielded(duration) => Some(duration),
            GeneratorState::Complete(()) => {
                self.completed = true;
                None
            }
        }
    }
}
