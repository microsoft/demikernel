// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::IoUringRuntime;
use ::runtime::{
    task::SchedulerRuntime,
    timer::{
        TimerRc,
        WaitFuture,
    },
};
use ::scheduler::{
    SchedulerFuture,
    SchedulerHandle,
};
use ::std::time::Instant;

//==============================================================================
// Trait Implementations
//==============================================================================

/// Scheduler runtime trait implementation for I/O user ring runtime.
impl SchedulerRuntime for IoUringRuntime {
    type WaitFuture = WaitFuture<TimerRc>;

    /// Creates a future on which one should wait a given duration.
    fn wait(&self, duration: std::time::Duration) -> Self::WaitFuture {
        let now: Instant = self.timer.now();
        self.timer.wait_until(self.timer.clone(), now + duration)
    }

    /// Creates a future on which one should until a given point in time.
    fn wait_until(&self, when: std::time::Instant) -> Self::WaitFuture {
        self.timer.wait_until(self.timer.clone(), when)
    }

    /// Returns the current runtime clock.
    fn now(&self) -> std::time::Instant {
        self.timer.now()
    }

    /// Advances the runtime clock to some point in time.
    fn advance_clock(&self, now: Instant) {
        self.timer.advance_clock(now);
    }

    /// Spawns a new task.
    fn spawn<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => panic!("failed to insert future in the scheduler"),
        }
    }

    /// Schedules a task for execution.
    fn schedule<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => panic!("failed to insert future in the scheduler"),
        }
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
