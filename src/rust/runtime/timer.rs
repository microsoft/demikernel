// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! This module implements a global timer for the Demikernel system. In order to keep the networking stack and other
//! parts of the system deterministic, we control time and time out events from thisn single file.

//======================================================================================================================
// Imports
//======================================================================================================================
use crate::{
    expect_some,
    runtime::SharedObject,
};
use ::core::cmp::Reverse;
use ::std::{
    collections::BinaryHeap,
    future::Future,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
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

//======================================================================================================================
// Thread local variable
//======================================================================================================================

thread_local! {
/// This is our shared sense of time. It is explicitly moved forward ONLY by the runtime and used to trigger time outs.
static THREAD_TIME: SharedTimer = SharedTimer::default();
}

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Eq, PartialEq)]
/// The state of the coroutine using this condition variable.
enum YieldState {
    Running,
    Yielded(YieldPointId),
}

#[derive(Eq, PartialEq, Clone, Copy)]
struct YieldPointId(u64);

struct YieldPoint {
    /// The time out.
    expiry: Instant,
    /// State of the yield.
    state: YieldState,
}

struct TimerQueueEntry {
    expiry: Instant,
    id: YieldPointId,
    waker: Waker,
}

/// Timer that holds one or more events for future wake up.
pub struct Timer {
    now: Instant,
    // Use a reverse to get a min heap.
    heap: BinaryHeap<Reverse<TimerQueueEntry>>,
    // Monotonically increasing identifier for yield points.
    last_id: YieldPointId,
}

#[derive(Clone)]
pub struct SharedTimer(SharedObject<Timer>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl YieldPointId {
    pub fn increment(&mut self) {
        self.0 = self.0 + 1;
    }
}

impl SharedTimer {
    /// This sets the time but is only used for initialization.
    fn set_time(&mut self, now: Instant) {
        // Clear out existing timers because they are meaningless once time has been moved in a non-monotonically
        // increasing manner.
        self.heap.clear();
        self.now = now;
    }

    fn advance_clock(&mut self, now: Instant) {
        assert!(self.now <= now);
        while let Some(Reverse(entry)) = self.heap.peek() {
            if now < entry.expiry {
                break;
            }
            let entry: TimerQueueEntry =
                expect_some!(self.heap.pop(), "should have an entry because we were able to peek").0;
            entry.waker.wake_by_ref();
        }
        self.now = now;
    }

    fn now(&self) -> Instant {
        self.now
    }

    fn add_timeout(&mut self, expiry: Instant, waker: Waker) -> YieldPointId {
        let id = self.last_id;
        self.last_id.increment();

        let entry: TimerQueueEntry = TimerQueueEntry { expiry, id, waker };
        self.heap.push(Reverse(entry));
        id
    }

    fn remove_timeout(&mut self, id: YieldPointId) {
        self.heap.retain(|entry| entry.0.id != id);
    }
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Sets the global time in the Demikernel system to [now].
pub fn global_set_time(now: Instant) {
    THREAD_TIME.with(|s| {
        s.clone().set_time(now);
    })
}

/// Causes global time in the Demikernel system to move forward and triggers all timeouts that have passed.
pub fn global_advance_clock(now: Instant) {
    THREAD_TIME.with(|s| {
        s.clone().advance_clock(now);
    })
}

/// Gets the current global time in the Demikernel system.
pub fn global_get_time() -> Instant {
    THREAD_TIME.with(|s| s.now())
}

/// Blocks until the system time moves
pub async fn wait(timeout: Duration) {
    let now: Instant = global_get_time();
    wait_until(now + timeout).await
}

pub async fn wait_until(expiry: Instant) {
    YieldPoint {
        expiry,
        state: YieldState::Running,
    }
    .await
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Default for SharedTimer {
    fn default() -> Self {
        Self(SharedObject::<Timer>::new(Timer {
            now: Instant::now(),
            heap: BinaryHeap::new(),
            last_id: YieldPointId(0),
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

impl Future for YieldPoint {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let self_: &mut Self = self.get_mut();
        match self_.state {
            YieldState::Running => {
                // If the timer expired while we were running and before we yielded, just return.
                if self_.expiry <= global_get_time() {
                    Poll::Ready(())
                } else {
                    let id: YieldPointId =
                        THREAD_TIME.with(|s| s.clone().add_timeout(self_.expiry, context.waker().clone()));
                    self_.state = YieldState::Yielded(id);
                    Poll::Pending
                }
            },
            YieldState::Yielded(_) => {
                if global_get_time() >= self_.expiry {
                    Poll::Ready(())
                } else {
                    // Spurious wake up because we wake all blocked yield points in a task.
                    Poll::Pending
                }
            },
        }
    }
}

impl Drop for YieldPoint {
    fn drop(&mut self) {
        if let YieldState::Yielded(id) = self.state {
            THREAD_TIME.with(|s| s.clone().remove_timeout(id));
        }
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
