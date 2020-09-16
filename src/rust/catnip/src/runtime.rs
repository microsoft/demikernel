// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{prelude::*, rand::Rng};
use rand_core::SeedableRng;
use std::future::Future;
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};
use std::collections::BinaryHeap;
use std::cmp::Ordering;
use std::task::Waker;
use std::pin::Pin;
use std::task::{Poll, Context};
use std::collections::binary_heap::PeekMut;

// TODO: Switch to unsync versions
use futures::channel::oneshot::{channel, Sender};
use futures::FutureExt;

struct Record {
    when: Instant,
    tx: Sender<()>,
}

impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        self.when.eq(&other.when)
    }
}

impl Eq for Record {
}

impl Ord for Record {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse the order since `BinaryHeap` is a max-heap
        other.when.cmp(&self.when)
    }
}

impl PartialOrd for Record {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub type WaitFuture = impl Future<Output = ()>;

pub struct Timer {
    now: Instant,
    heap: BinaryHeap<Record>,
    waker: Option<Waker>,
}

impl Timer {
    pub fn new(now: Instant) -> Self {
        Self { now, heap: BinaryHeap::new(), waker: None }
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    pub fn wait_until(&mut self, when: Instant) -> WaitFuture {
        let (tx, rx) = channel();
        if when <= self.now {
            let _ = tx.send(());
        } else {
            self.heap.push(Record { when, tx });
        }
        rx.map(|_| ())
    }

    pub fn advance_clock(&mut self, now: Instant) {
        assert!(now >= self.now, "{:?} vs. {:?}", self.now, now);
        if let Some(record) = self.heap.peek() {
            if record.when <= now {
                if let Some(w) = self.waker.take() {
                    w.wake();
                }
            }
        }
        self.now = now;
    }
}

impl Future for Timer {
    type Output = !;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let now = self.now;
        while let Some(record) = self.heap.peek_mut() {
            if record.when > now {
                break;
            }
            let record = PeekMut::pop(record);
            let _ = record.tx.send(());
        }
        self.waker = Some(ctx.waker().clone());
        Poll::Pending
    }
}

#[derive(Clone)]
pub struct Runtime {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    events: VecDeque<Rc<Event>>,
    options: Options,
    rng: Rng,
    timer: Timer,
}

impl Runtime {
    pub fn from_options(now: Instant, options: Options) -> Self {
        let rng = Rng::from_seed(options.rng_seed);
        let inner = Inner {
            events: VecDeque::new(),
            options,
            rng,
            timer: Timer::new(now),
        };
        Self { inner: Rc::new(RefCell::new(inner)) }
    }

    pub fn options(&self) -> Options {
        self.inner.borrow().options.clone()
    }

    pub fn now(&self) -> Instant {
        self.inner.borrow().timer.now
    }

    pub fn emit_event(&self, event: Event) {
        let mut inner = self.inner.borrow_mut();
        info!(
            "event emitted for {} (len is now {}) => {:?}",
            inner.options.my_ipv4_addr,
            inner.events.len() + 1,
            event
        );
        inner.events.push_back(Rc::new(event));
    }

    pub fn with_rng<R>(&self, f: impl FnOnce(&mut Rng) -> R) -> R {
        let mut inner = self.inner.borrow_mut();
        f(&mut inner.rng)
    }

    pub fn advance_clock(&self, now: Instant) {
        let mut inner = self.inner.borrow_mut();
        inner.timer.advance_clock(now)
    }

    pub fn next_event(&self) -> Option<Rc<Event>> {
        let inner = self.inner.borrow();
        inner.events.front().cloned()
    }

    pub fn pop_event(&self) -> Option<Rc<Event>> {
        let mut inner = self.inner.borrow_mut();
        if let Some(event) = inner.events.pop_front() {
            info!(
                "event popped for {} (len is now {}) => {:?}",
                inner.options.my_ipv4_addr,
                inner.events.len(),
                event
            );
            Some(event)
        } else {
            None
        }
    }

    pub fn wait(&self, how_long: Duration) -> impl Future<Output = ()> {
        let mut inner = self.inner.borrow_mut();
        let when = inner.timer.now + how_long;
        inner.timer.wait_until(when)
    }

    pub fn wait_until(&self, when: Instant) -> impl Future<Output = ()> {
        let mut inner = self.inner.borrow_mut();
        inner.timer.wait_until(when)
    }

    pub fn exponential_retry<F: Future>(
        &self,
        mut timeout: Duration,
        max_attempts: usize,
        mut f: impl FnMut() -> F,
    ) -> impl Future<Output=Result<F::Output>>
    {
        let self_ = self.clone();
        async move {
            assert!(max_attempts > 0);
            for _ in 0..max_attempts {
                futures::select! {
                    r = f().fuse() => {
                        return Ok(r);
                    },
                    _ = self_.wait(timeout).fuse() => {
                        timeout *= 2
                    },
                }
            }
            Err(Fail::Timeout {})
        }
    }
}

impl Future for Runtime {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        let mut inner = self.get_mut().inner.borrow_mut();
        Future::poll(Pin::new(&mut inner.timer), ctx)
    }
}
