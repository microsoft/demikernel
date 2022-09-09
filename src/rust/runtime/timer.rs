// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::collections::intrusive::pairing_heap::{
    HeapNode,
    PairingHeap,
};
use ::futures::future::FusedFuture;
use ::std::{
    cell::RefCell,
    future::Future,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Traits
//==============================================================================

pub trait TimerPtr: Sized {
    fn timer(&self) -> &Timer<Self>;
}

//==============================================================================
// Enumerations
//==============================================================================

enum PollState {
    Unregistered,
    Registered,
    Expired,
}

//==============================================================================
// Structures
//==============================================================================

struct TimerQueueEntry {
    expiry: Instant,
    task: Option<Waker>,
    state: PollState,
}

struct TimerInner {
    now: Instant,
    heap: PairingHeap<TimerQueueEntry>,
}

pub struct Timer<P: TimerPtr> {
    inner: RefCell<TimerInner>,
    _marker: PhantomData<P>,
}

#[derive(Clone)]
pub struct TimerRc(pub Rc<Timer<TimerRc>>);

pub struct WaitFuture<P: TimerPtr> {
    ptr: Option<P>,
    wait_node: HeapNode<TimerQueueEntry>,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl<P: TimerPtr> Timer<P> {
    pub fn new(now: Instant) -> Self {
        let inner = TimerInner {
            now,
            heap: PairingHeap::new(),
        };
        Self {
            inner: RefCell::new(inner),
            _marker: PhantomData,
        }
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

    pub fn wait(&self, ptr: P, timeout: Duration) -> WaitFuture<P> {
        self.wait_until(ptr, self.now() + timeout)
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

//==============================================================================
// Trait Implementations
//==============================================================================

impl Deref for TimerRc {
    type Target = Rc<Timer<TimerRc>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TimerPtr for TimerRc {
    fn timer(&self) -> &Timer<Self> {
        &*self.0
    }
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
    fn partial_cmp(&self, other: &TimerQueueEntry) -> Option<core::cmp::Ordering> {
        // Compare timer queue entries by expiration time
        self.expiry.partial_cmp(&other.expiry)
    }
}

impl Ord for TimerQueueEntry {
    fn cmp(&self, other: &TimerQueueEntry) -> core::cmp::Ordering {
        self.expiry.cmp(&other.expiry)
    }
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
                PollState::Expired => Poll::Ready(()),
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

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use super::{
        Timer,
        TimerRc,
    };
    use futures::task::noop_waker_ref;
    use std::{
        future::Future,
        pin::Pin,
        rc::Rc,
        task::Context,
        time::{
            Duration,
            Instant,
        },
    };

    #[test]
    fn test_timer() {
        let mut ctx = Context::from_waker(noop_waker_ref());
        let mut now = Instant::now();

        let timer = TimerRc(Rc::new(Timer::new(now)));

        let wait_future1 = timer.wait(timer.clone(), Duration::from_secs(2));
        futures::pin_mut!(wait_future1);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());

        now += Duration::from_millis(500);
        timer.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        let wait_future2 = timer.wait(timer.clone(), Duration::from_secs(1));
        futures::pin_mut!(wait_future2);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_pending());

        now += Duration::from_millis(500);
        timer.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_pending());

        now += Duration::from_millis(500);
        timer.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending());
        assert!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_ready());

        now += Duration::from_millis(750);
        timer.advance_clock(now);

        assert!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready());
    }
}
