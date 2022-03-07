// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::LinuxRuntime;
use ::catwalk::{
    SchedulerFuture,
    SchedulerHandle,
};
use ::runtime::{
    task::SchedulerRuntime,
    timer::{
        TimerRc,
        WaitFuture,
    },
};
use ::std::time::{
    Duration,
    Instant,
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Scheduler Runtime Trait Implementation for Linux Runtime
impl SchedulerRuntime for LinuxRuntime {
    type WaitFuture = WaitFuture<TimerRc>;

    /// Creates a future on which one should wait a given duration.
    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let now: Instant = self.timer.now();
        self.timer.wait_until(self.timer.clone(), now + duration)
    }

    /// Creates a future on which one should until a given point in time.
    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        self.timer.wait_until(self.timer.clone(), when)
    }

    /// Returns the current runtime clock.
    fn now(&self) -> Instant {
        self.timer.now()
    }

    /// Advances the runtime clock to some point in time.
    fn advance_clock(&self, now: Instant) {
        self.timer.advance_clock(now);
    }

    /// Spawns a new task.
    fn spawn<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        self.scheduler.insert(future)
    }

    /// Schedules a task for execution.
    fn schedule<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        self.scheduler.insert(future)
    }

    /// Gets the handle of a task.
    fn get_handle(&self, key: u64) -> Option<SchedulerHandle> {
        self.scheduler.from_raw_handle(key)
    }

    /// Takes out a given task from the scheduler.
    fn take(&self, handle: SchedulerHandle) -> Box<dyn SchedulerFuture> {
        self.scheduler.take(handle)
    }

    /// Polls the scheduler.
    fn poll(&self) {
        self.scheduler.poll()
    }
}
