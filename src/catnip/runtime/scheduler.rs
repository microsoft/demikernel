// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::DPDKRuntime;
use ::runtime::{
    scheduler::{
        SchedulerFuture,
        SchedulerHandle,
    },
    task::SchedulerRuntime,
    timer::{
        Timer,
        TimerPtr,
        WaitFuture,
    },
};
use ::std::{
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Structures
//==============================================================================

#[derive(Clone)]
pub struct TimerRc(pub Rc<Timer<TimerRc>>);

//==============================================================================
// Trait Implementations
//==============================================================================

impl TimerPtr for TimerRc {
    fn timer(&self) -> &Timer<Self> {
        &*self.0
    }
}

/// Scheduler Runtime Trait Implementation for DPDK Runtime
impl SchedulerRuntime for DPDKRuntime {
    type WaitFuture = WaitFuture<TimerRc>;

    fn advance_clock(&self, now: Instant) {
        self.timer.0.advance_clock(now);
    }

    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let now = self.timer.0.now();
        self.timer.0.wait_until(self.timer.clone(), now + duration)
    }

    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        self.timer.0.wait_until(self.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.timer.0.now()
    }

    fn spawn<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => panic!("failed to insert future in the scheduler"),
        }
    }

    fn schedule<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => panic!("failed to insert future in the scheduler"),
        }
    }

    fn get_handle(&self, key: u64) -> Option<SchedulerHandle> {
        self.scheduler.from_raw_handle(key)
    }

    fn take(&self, handle: SchedulerHandle) -> Box<dyn SchedulerFuture> {
        self.scheduler.take(handle)
    }

    fn poll(&self) {
        self.scheduler.poll()
    }
}
