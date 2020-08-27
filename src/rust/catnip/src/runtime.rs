// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{prelude::*, r#async, rand::Rng};
use rand_core::SeedableRng;
use std::{
    cell::{RefCell, RefMut},
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Runtime<'a> {
    events: Rc<RefCell<VecDeque<Rc<Event>>>>,
    options: Rc<Options>,
    r#async: r#async::Runtime<'a>,
    rng: Rc<RefCell<Rng>>,
}

impl<'a> Runtime<'a> {
    pub fn from_options(now: Instant, options: Options) -> Runtime<'a> {
        let rng = Rng::from_seed(options.rng_seed);
        Runtime {
            events: Rc::new(RefCell::new(VecDeque::new())),
            options: Rc::new(options),
            r#async: r#async::Runtime::new(now),
            rng: Rc::new(RefCell::new(rng)),
        }
    }

    pub fn options(&self) -> Rc<Options> {
        self.options.clone()
    }

    pub fn now(&self) -> Instant {
        self.r#async.clock()
    }

    pub fn emit_event(&self, event: Event) {
        let mut events = self.events.borrow_mut();
        info!(
            "event emitted for {} (len is now {}) => {:?}",
            self.options.my_ipv4_addr,
            events.len() + 1,
            event
        );
        events.push_back(Rc::new(event));
    }

    pub fn rng_mut(&self) -> RefMut<Rng> {
        self.rng.borrow_mut()
    }

    pub fn advance_clock(&self, now: Instant) {
        while self.r#async.poll(now).is_some() {}
    }

    pub fn next_event(&self) -> Option<Rc<Event>> {
        self.events.borrow().front().cloned()
    }

    pub fn pop_event(&self) -> Option<Rc<Event>> {
        let mut events = self.events.borrow_mut();
        if let Some(event) = events.pop_front() {
            info!(
                "event popped for {} (len is now {}) => {:?}",
                self.options.my_ipv4_addr,
                events.len(),
                event
            );
            Some(event)
        } else {
            None
        }
    }

    pub fn wait(&self, _how_long: Duration) -> impl std::future::Future<Output = ()> {
        async {
            unimplemented!();
        }
    }

    pub fn wait_until(&self, _when: Instant) -> impl std::future::Future<Output = ()> {
        async {
            unimplemented!();
        }
    }
}
