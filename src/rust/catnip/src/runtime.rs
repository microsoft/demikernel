// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{prelude::*, r#async, rand::Rng};
use rand_core::SeedableRng;
use std::future::Future;
use std::{
    cell::{RefCell, RefMut},
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
use pin_cell::{PinCell, PinMut};
use pin_project::pin_project;

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

struct Timer {
    now: Instant,
    heap: BinaryHeap<Record>,
    waker: Option<Waker>,
}

impl Timer {
    fn new(now: Instant) -> Self {
        Self { now, heap: BinaryHeap::new(), waker: None }
    }

    fn timer(&mut self, when: Instant) -> impl Future<Output = ()> {
        let (tx, rx) = channel();
        if when <= self.now {
            let _ = tx.send(());
        } else {
            self.heap.push(Record { when, tx });
        }
        rx.map(|_| ())
    }

    fn advance_time(&mut self, now: Instant) {
        assert!(now > self.now);
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
    inner: Pin<Rc<Inner>>,
}

#[pin_project]
struct Inner {
    events: RefCell<VecDeque<Rc<Event>>>,
    options: Options,
    rng: RefCell<Rng>,

    #[pin]
    timer: PinCell<Timer>,
}

impl Runtime {
    pub fn from_options(now: Instant, options: Options) -> Self {
        let rng = Rng::from_seed(options.rng_seed);
        let inner = Inner {
            events: RefCell::new(VecDeque::new()),
            options,
            rng: RefCell::new(rng),
            timer: PinCell::new(Timer::new(now)),
        };
        Self { inner: Pin::new(Rc::new(inner)) }
    }

    pub fn options(&self) -> Options {
        self.inner.as_ref().options.clone()
    }

    pub fn now(&self) -> Instant {
        self.inner.as_ref().timer.borrow().now
    }

    pub fn emit_event(&self, event: Event) {
        let mut events = self.inner.as_ref().events.borrow_mut();
        info!(
            "event emitted for {} (len is now {}) => {:?}",
            self.options().my_ipv4_addr,
            events.len() + 1,
            event
        );
        events.push_back(Rc::new(event));
    }

    pub fn with_rng<R>(&self, f: impl FnOnce(&mut Rng) -> R) -> R {
        let mut rng = self.inner.as_ref().rng.borrow_mut();
        f(&mut *rng)
    }

    pub fn advance_clock(&self, _now: Instant) {
        // let mut timer_ref = PinCell::borrow_mut(self.inner.as_ref().project_ref().timer);
        // let _: () = PinMut::as_mut(&mut timer_ref);
    }

    pub fn next_event(&self) -> Option<Rc<Event>> {
        self.inner.as_ref().events.borrow().front().cloned()
    }

    pub fn pop_event(&self) -> Option<Rc<Event>> {
        let mut events = self.inner.as_ref().events.borrow_mut();
        if let Some(event) = events.pop_front() {
            info!(
                "event popped for {} (len is now {}) => {:?}",
                self.options().my_ipv4_addr,
                events.len(),
                event
            );
            Some(event)
        } else {
            None
        }
    }

    pub fn wait(&self, _how_long: Duration) -> impl Future<Output = ()> {
        async {
            unimplemented!();
        }
    }

    pub fn wait_until(&self, _when: Instant) -> impl Future<Output = ()> {
        async {
            unimplemented!();
        }
    }
}

impl Future for Runtime {
    type Output = !;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<!> {
        // XXX: This is a mouthful.
        let mut timer_ref = PinCell::borrow_mut(self.into_ref().get_ref().inner.as_ref().project_ref().timer);
        Future::poll(PinMut::as_mut(&mut timer_ref), ctx)
    }
}
