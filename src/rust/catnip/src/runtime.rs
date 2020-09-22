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
use futures::future::FusedFuture;
use futures_intrusive::intrusive_pairing_heap::{HeapNode, PairingHeap};

enum PollState {
    Unregistered,
    Registered,
    Expired,
}

struct TimerQueueEntry {
    expiry: Instant,
    task: Option<Waker>,
    state: PollState,
}

impl PartialEq for TimerQueueEntry {
    fn eq(&self, other: &TimerQueueEntry) -> bool {
        // This is technically not correct. However for the usage in this module
        // we only need to compare timers by expiration.
        self.expiry == other.expiry
    }
}

impl Eq for TimerQueueEntry {}

impl PartialOrd for TimerQueueEntry {
    fn partial_cmp(
        &self,
        other: &TimerQueueEntry,
    ) -> Option<core::cmp::Ordering> {
        // Compare timer queue entries by expiration time
        self.expiry.partial_cmp(&other.expiry)
    }
}

impl Ord for TimerQueueEntry {
    fn cmp(&self, other: &TimerQueueEntry) -> core::cmp::Ordering {
        self.expiry.cmp(&other.expiry)
    }
}

struct TimerInner2 {
    now: Instant,
    heap: PairingHeap<TimerQueueEntry>,
}

pub struct Timer2 {
    inner: RefCell<TimerInner2>,
}

impl Timer2 {
    fn new(now: Instant) -> Self {
        let inner = TimerInner2 {
            now,
            heap: PairingHeap::new(),
        };
        Self { inner: RefCell::new(inner) }
    }

    fn advance_clock(&self, now: Instant) {
        let mut inner = self.inner.borrow_mut();
        assert!(inner.now <= now);

        while let Some(mut first) = inner.heap.peek_min() {
            unsafe {
                let entry = first.as_mut();
                let first_expiry = entry.expiry;
                if now < first_expiry {
                    break;
                }

                entry.state = PollState::Expired;
                if let Some(task) = entry.task.take() {
                    task.wake();
                }
                inner.heap.remove(entry);
            }
        }
    }

    fn now(&self) -> Instant {
        self.inner.borrow().now
    }

    fn wait_until(&self, rt: Rc<Inner>, expiry: Instant) -> WaitFuture2 {
        let entry = TimerQueueEntry {
            expiry,
            task: None,
            state: PollState::Unregistered,
        };
        WaitFuture2 {
            rt: Some(rt),
            wait_node: HeapNode::new(entry),
        }
    }
}

pub struct WaitFuture2 {
    rt: Option<Rc<Inner>>,
    wait_node: HeapNode<TimerQueueEntry>,
}

impl Future for WaitFuture2 {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut_self: &mut Self = unsafe { Pin::get_unchecked_mut(self) };

        let result = {
            let rt = mut_self.rt.as_ref().expect("Polled future after completion");
            let timer = &rt.timer;

            let mut inner = timer.inner.borrow_mut();
            let wait_node = &mut mut_self.wait_node;

            match wait_node.state {
                PollState::Unregistered => {
                    if inner.now >= wait_node.expiry {
                        wait_node.state = PollState::Expired;
                        Poll::Ready(())
                    } else {
                        wait_node.task = Some(cx.waker().clone());
                        wait_node.state = PollState::Registered;
                        unsafe {
                            inner.heap.insert(wait_node);
                        }
                        Poll::Pending
                    }
                },
                PollState::Registered => {
                    if wait_node.task.as_ref().map_or(true, |w| !w.will_wake(cx.waker())) {
                        wait_node.task = Some(cx.waker().clone());
                    }
                    Poll::Pending
                },
                PollState::Expired => Poll::Ready(())
            }
        };
        if result.is_ready() {
            mut_self.rt = None;
        }
        result
    }
}

impl FusedFuture for WaitFuture2 {
    fn is_terminated(&self) -> bool {
        self.rt.is_none()
    }
}

impl Drop for WaitFuture2 {
    fn drop(&mut self) {
        // If this TimerFuture has been polled and it was added to the
        // wait queue at the timer, it must be removed before dropping.
        // Otherwise the timer would access invalid memory.
        if let Some(rt) = &self.rt {
            if let PollState::Registered = self.wait_node.state {
                unsafe { rt.timer.inner.borrow_mut().heap.remove(&mut self.wait_node) };
                self.wait_node.state = PollState::Unregistered;
            }
        }
    }
}



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
    inner: Rc<Inner>,
}

struct Inner {
    events: RefCell<VecDeque<Rc<Event>>>,
    options: Options,
    rng: RefCell<Rng>,
    timer: Timer2,
}

impl Runtime {
    pub fn from_options(now: Instant, options: Options) -> Self {
        let rng = Rng::from_seed(options.rng_seed);
        let inner = Inner {
            events: RefCell::new(VecDeque::new()),
            options,
            rng: RefCell::new(rng),
            timer: Timer2::new(now),
        };
        Self { inner: Rc::new(inner) }
    }

    pub fn options(&self) -> Options {
        self.inner.options.clone()
    }

    pub fn now(&self) -> Instant {
        self.inner.timer.now()
    }

    pub fn emit_event(&self, event: Event) {
        let mut events = self.inner.events.borrow_mut();
        info!("event emitted => {:?}", event);
        events.push_back(Rc::new(event));
    }

    pub fn with_rng<R>(&self, f: impl FnOnce(&mut Rng) -> R) -> R {
        let mut rng = self.inner.rng.borrow_mut();
        f(&mut *rng)
    }

    pub fn advance_clock(&self, now: Instant) {
        self.inner.timer.advance_clock(now);
    }

    pub fn next_event(&self) -> Option<Rc<Event>> {
        let events = self.inner.events.borrow();
        events.front().cloned()
    }

    pub fn pop_event(&self) -> Option<Rc<Event>> {
        let mut events = self.inner.events.borrow_mut();
        if let Some(event) = events.pop_front() {
            info!("event popped => {:?}", event);
            Some(event)
        } else {
            None
        }
    }

    pub fn wait(&self, how_long: Duration) -> WaitFuture2 {
        let when = self.inner.timer.now() + how_long;
        self.inner.timer.wait_until(self.inner.clone(), when)
    }

    pub fn wait_until(&self, when: Instant) -> WaitFuture2 {
        self.inner.timer.wait_until(self.inner.clone(), when)
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
