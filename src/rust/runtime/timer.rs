// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::{
    SharedConditionVariable,
    SharedObject,
};
use ::core::cmp::Reverse;
use ::std::{
    collections::BinaryHeap,
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
    cond_var: SharedConditionVariable,
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
    /// This sets the time but is only used for initialization.
    pub fn set_time(&mut self, now: Instant) {
        self.now = now;
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
            entry.cond_var.broadcast();
        }
        self.now = now;
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    pub async fn wait(self, timeout: Duration, cond_var: SharedConditionVariable) {
        let now: Instant = self.now;
        self.wait_until(now + timeout, cond_var).await
    }

    pub async fn wait_until(mut self, expiry: Instant, cond_var: SharedConditionVariable) {
        let entry = TimerQueueEntry {
            expiry,
            cond_var: cond_var.clone(),
        };
        self.heap.push(Reverse(entry));
        while self.now < expiry {
            cond_var.wait().await;
        }
    }
}

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

// FIXME: Turning these off because they do not use the scheduler.

// #[cfg(test)]
// mod tests {
//     use super::{
//         SharedConditionVariable,
//         SharedTimer,
//     };
//     use ::anyhow::Result;
//     use futures::task::noop_waker_ref;
//     use std::{
//         future::Future,
//         pin::Pin,
//         task::Context,
//         time::{
//             Duration,
//             Instant,
//         },
//     };

//     #[test]
//     fn test_timer() -> Result<()> {
//         let mut ctx = Context::from_waker(noop_waker_ref());
//         let mut now = Instant::now();

//         // Add a single time out at start of test + 2 seconds.
//         let mut timer: SharedTimer = SharedTimer::new(now);
//         let cond_var: SharedConditionVariable = SharedConditionVariable::default();
//         let wait_future1 = timer.clone().wait(Duration::from_secs(2), cond_var);
//         futures::pin_mut!(wait_future1);

//         // Check that the time out has not triggered.
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), false);

//         // Move time to start of test + 0.5 seconds.
//         now += Duration::from_millis(500);
//         timer.advance_clock(now);

//         // Check that the first time out has not triggered.
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), false);

//         // Create second time out at start of test + 1.5 seconds.
//         let cond_var2: SharedConditionVariable = SharedConditionVariable::default();
//         let wait_future2 = timer.clone().wait(Duration::from_secs(1), cond_var2);
//         futures::pin_mut!(wait_future2);

//         // Check that both have not triggered.
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), false);
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_ready(), false);

//         // Move time to start of test + 1 second.
//         now += Duration::from_millis(500);
//         timer.advance_clock(now);

//         // Check that both time outs have not triggered
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), false);
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_ready(), false);

//         // Create a new timeout for start of test + 5 seconds.
//         let mut cond_var3: SharedConditionVariable = SharedConditionVariable::default();
//         let wait_future3 = timer.clone().wait(Duration::from_secs(4), cond_var3.clone());
//         futures::pin_mut!(wait_future3);

//         // Move time to start of test + 1.5 seconds.
//         now += Duration::from_millis(500);
//         timer.advance_clock(now);

//         // Check that timer2 has triggered but not timer1.
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), false);
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future2), &mut ctx).is_ready(), true);
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future3), &mut ctx).is_ready(), false);

//         // Move time to start of test + 2.15 seconds.
//         now += Duration::from_millis(750);
//         timer.advance_clock(now);

//         // Check that timer1 has triggered.
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future1), &mut ctx).is_ready(), true);
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future3), &mut ctx).is_ready(), false);

//         // Cancel the condition variable waiters and ensure that the timer does not fire.
//         cond_var3.cancel();
//         now += Duration::from_millis(10000);
//         crate::ensure_eq!(Future::poll(Pin::new(&mut wait_future3), &mut ctx).is_ready(), false);

//         Ok(())
//     }
// }
