// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::marker::PhantomData;
use crate::{prelude::*, rand::Rng};
use rand_core::SeedableRng;
use std::future::Future;
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};
use std::task::Waker;
use std::pin::Pin;
use std::task::{Poll, Context};
use futures::FutureExt;
use futures::future::FusedFuture;
use futures_intrusive::intrusive_pairing_heap::{HeapNode, PairingHeap};

pub trait TimerPtr: Sized {
    fn timer(&self) -> &Timer<Self>;
}

impl TimerPtr for Runtime {
    fn timer(&self) -> &Timer<Self> {
        &self.inner.timer
    }
}

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

struct TimerInner {
    now: Instant,
    heap: PairingHeap<TimerQueueEntry>,
}

pub struct Timer<P: TimerPtr> {
    inner: RefCell<TimerInner>,
    _marker: PhantomData<P>,
}

impl<P: TimerPtr> Timer<P> {
    pub fn new(now: Instant) -> Self {
        let inner = TimerInner {
            now,
            heap: PairingHeap::new(),
        };
        Self { inner: RefCell::new(inner), _marker: PhantomData }
    }

    pub fn advance_clock(&self, now: Instant) {
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
        inner.now = now;
    }

    pub fn now(&self) -> Instant {
        self.inner.borrow().now
    }

    pub fn wait_until(&self, ptr: P, expiry: Instant) -> WaitFuture<P> {
        let entry = TimerQueueEntry {
            expiry,
            task: None,
            state: PollState::Unregistered,
        };
        WaitFuture {
            ptr: Some(ptr),
            wait_node: HeapNode::new(entry),
        }
    }
}

pub type RuntimeWaitFuture = WaitFuture<Runtime>;

pub struct WaitFuture<P: TimerPtr> {
    ptr: Option<P>,
    wait_node: HeapNode<TimerQueueEntry>,
}

impl<P: TimerPtr> Future for WaitFuture<P> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut_self: &mut Self = unsafe { Pin::get_unchecked_mut(self) };

        let result = {
            let ptr = mut_self.ptr.as_ref().expect("Polled future after completion");
            let timer = ptr.timer();

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
            mut_self.ptr = None;
        }
        result
    }
}

impl<P: TimerPtr> FusedFuture for WaitFuture<P> {
    fn is_terminated(&self) -> bool {
        self.ptr.is_none()
    }
}

impl<P: TimerPtr> Drop for WaitFuture<P> {
    fn drop(&mut self) {
        // If this TimerFuture has been polled and it was added to the
        // wait queue at the timer, it must be removed before dropping.
        // Otherwise the timer would access invalid memory.
        if let Some(ptr) = &self.ptr {
            if let PollState::Registered = self.wait_node.state {
                unsafe { ptr.timer().inner.borrow_mut().heap.remove(&mut self.wait_node) };
                self.wait_node.state = PollState::Unregistered;
            }
        }
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
    timer: Timer<Runtime>,
}

impl Runtime {
    pub fn from_options(now: Instant, options: Options) -> Self {
        let rng = Rng::from_seed(options.rng_seed);
        let inner = Inner {
            events: RefCell::new(VecDeque::new()),
            options,
            rng: RefCell::new(rng),
            timer: Timer::new(now),
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

    pub fn wait(&self, how_long: Duration) -> RuntimeWaitFuture {
        let when = self.inner.timer.now() + how_long;
        self.inner.timer.wait_until(self.clone(), when)
    }

    pub fn wait_until(&self, when: Instant) -> RuntimeWaitFuture {
        self.inner.timer.wait_until(self.clone(), when)
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

#[cfg(test)]
mod tests {
    use super::Runtime;
    use crate::options::Options;
    use std::time::{Duration, Instant};
    use std::task::{Context};
    use std::future::Future;
    use futures::task::noop_waker_ref;
    use std::pin::Pin;

    #[test]
    fn test_timer() {
        let mut ctx = Context::from_waker(noop_waker_ref());
        let mut now = Instant::now();

        let rt = Runtime::from_options(now, Options::default());

        let wait_future1 = rt.wait(Duration::from_secs(2));
        futures::pin_mut!(wait_future1);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());

        now += Duration::from_millis(500);
        rt.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        let wait_future2 = rt.wait(Duration::from_secs(1));
        futures::pin_mut!(wait_future2);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_pending());

        now += Duration::from_millis(500);
        rt.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_pending());

        now += Duration::from_millis(500);
        rt.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_ready());

        now += Duration::from_millis(750);
        rt.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready());
    }
}
