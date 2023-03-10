// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// TODO: Replace with coroutine eventually.
pub mod coroutine;
mod future;
// TODO: Replace with task handle
mod handle;
mod page;
mod pin_slab;
// TODO: Replace with task
pub mod demi_scheduler;
mod result;
pub mod scheduler;
mod task;
mod waker64;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    coroutine::Coroutine,
    demi_scheduler::DemiScheduler,
    future::SchedulerFuture,
    handle::SchedulerHandle,
    result::FutureResult,
    scheduler::Scheduler,
    task::Task,
};
