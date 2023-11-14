// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    scheduler::{
        Yielder,
        YielderHandle,
    },
    Fail,
    SharedObject,
};
use ::async_trait::async_trait;
use ::core::cmp::Reverse;
use ::futures::{
    future::FusedFuture,
    FutureExt,
};
use ::std::{
    collections::BinaryHeap,
    future::Future,
    ops::{
        Deref,
        DerefMut,
    },
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Structures
//==============================================================================

struct TimerQueueEntry {
    expiry: Instant,
    yielder: YielderHandle,
}

/// Timer that holds one or more events for future wake up.
pub struct Timer {
    now: Instant,
    // Use a reverse to get a min heap.
    heap: BinaryHeap<Reverse<TimerQueueEntry>>,
}

#[derive(Clone)]
pub struct SharedTimer(SharedObject<Timer>);

//==============================================================================
// Associate Functions
//==============================================================================

impl SharedTimer {
    pub fn new(now: Instant) -> Self {
        Self(SharedObject::<Timer>::new(Timer {
            now,
            heap: BinaryHeap::new(),
        }))
    }

    pub fn advance_clock(&mut self, now: Instant) {
        assert!(self.now <= now);

        while let Some(Reverse(entry)) = self.heap.peek() {
            if now < entry.expiry {
                break;
            }
            let mut entry: TimerQueueEntry = self
                .heap
                .pop()
                .expect("should have an entry because we were able to peek")
                .0;
            entry.yielder.wake_with(Ok(()));
        }
        self.now = now;
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    pub async fn wait(self, timeout: Duration, yielder: &Yielder) -> Result<(), Fail> {
        let now: Instant = self.now;
        self.wait_until(now + timeout, &yielder).await
    }

    pub async fn wait_until(mut self, expiry: Instant, yielder: &Yielder) -> Result<(), Fail> {
        let entry = TimerQueueEntry {
            expiry,
            yielder: yielder.get_handle(),
        };
        self.heap.push(Reverse(entry));
        yielder.yield_until_wake().await
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

/// Provides useful high-level future-related methods.
#[async_trait(?Send)]
pub trait UtilityMethods: Future + FusedFuture + Unpin {
    /// Transforms our current future to include a timeout. We either return the results of the
    /// future finishing or a Timeout error. Whichever happens first.
    async fn with_timeout<Timer>(&mut self, timer: Timer) -> Result<Self::Output, Fail>
    where
        Timer: Future<Output = Result<(), Fail>>,
    {
        futures::select! {
            result = self => Ok(result),
            result = timer.fuse() => match result {
                Ok(()) => Err(Fail::new(libc::ETIMEDOUT, "timer expired")),
                Err(e) => Err(e),
            },
        }
    }
}

// Implement UtiliytMethods for any Future that implements Unpin and FusedFuture.
impl<F: ?Sized> UtilityMethods for F where F: Future + Unpin + FusedFuture {}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Default for SharedTimer {
    fn default() -> Self {
        Self(SharedObject::<Timer>::new(Timer {
            now: Instant::now(),
            heap: BinaryHeap::new(),
        }))
    }
}

impl Deref for SharedTimer {
    type Target = Timer;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedTimer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
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

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use super::SharedTimer;
    use crate::runtime::scheduler::Yielder;
    use ::anyhow::Result;
    use futures::task::noop_waker_ref;
    use std::{
        future::Future,
        pin::Pin,
        task::Context,
        time::{
            Duration,
            Instant,
        },
    };

    #[test]
    fn test_timer() -> Result<()> {
        let mut ctx = Context::from_waker(noop_waker_ref());
        let mut now = Instant::now();

        let mut timer: SharedTimer = SharedTimer::new(now);
        let timer_ref: SharedTimer = timer.clone();
        let yielder: Yielder = Yielder::new();

        let wait_future1 = timer_ref.wait(Duration::from_secs(2), &yielder);
        futures::pin_mut!(wait_future1);

        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending(), true);

        now += Duration::from_millis(500);
        timer.advance_clock(now);

        let timer_ref2: SharedTimer = timer.clone();
        let yielder2: Yielder = Yielder::new();
        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending(), true);
        let wait_future2 = timer_ref2.wait(Duration::from_secs(1), &yielder2);
        futures::pin_mut!(wait_future2);

        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending(), true);
        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_pending(), true);

        now += Duration::from_millis(500);
        timer.advance_clock(now);

        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending(), true);
        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_pending(), true);

        now += Duration::from_millis(500);
        timer.advance_clock(now);

        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_pending(), true);
        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_ready(), true);

        now += Duration::from_millis(750);
        timer.advance_clock(now);

        crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), true);

        Ok(())
    }
}
