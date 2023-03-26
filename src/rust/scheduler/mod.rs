// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! This module contains the Demikernel scheduler, which allocates CPU cycles to various Demikernel components. The
//! scheduler has the following hierarchy of dependency, ownership and resource (CPU) allocation. Under normal
//! operation, each layer gives its CPU cycles to the one below to perform some work: (1) the scheduler selects a
//! Task to run, (2) the Task runs its coroutine until it completes, (3) the coroutine runs until it cannot make
//! progress, then (4) the coroutine uses the Yielder to give cycles back to the Scheduler). The top and bottom layer
//! provide a Handle to check on and control the progress of execution. These handles are used under normal operation
//! but are also important in cases of non-normal operation (e.g., a socket is closed while an operation is pending on
//! it).
//!
//! (1)  Scheduler (scheduler.rs)                SchedulerHandle (handle.rs): check whether the Task has finished.
//!          | owns many
//!          V
//! (2)    Task (task.rs)
//!          | owns one
//!          V
//! (3)  coroutine (asyn fn from the libOS)
//!          | owns one
//!          V
//! (4)   Yielder (yielder.rs)                YielderHandle: Wake a yielded coroutine, Ok indicates more work, Fail
//!                                           indicates error and that the coroutine should exit gracefully.
//!
//! The [Task] structure is the primary unit of work in Demikernel. The [Scheduler] is responsible for scheduling [Task]
//! but it treats each [Task] simply as a unit that consumes CPU cycles until it completes.  The [SchedulerHandle] is
//! used to check whether a task is still running or complete and to take a task out of the scheduler once it stops. A
//! Task holds a single coroutine and runs it until it produces a result, then stores the result for later. Each
//! coroutine is an async function defined by a libOS, typically to perform either an I/O operation or a background
//! task. Coroutines that are capable of yielding when they are blocked contain a [Yielder] to give CPU cycles back to
//! the scheduler. The [YielderHandle] identifies a specific blocked coroutine and can be used to wake the coroutine.

mod handle;
mod page;
mod pin_slab;
pub mod scheduler;
pub mod task;
mod waker64;
pub mod yielder;

//==============================================================================
// Exports
//==============================================================================

pub use self::{
    handle::{
        SchedulerHandle,
        YielderHandle,
    },
    scheduler::Scheduler,
    task::{
        Task,
        TaskWithResult,
    },
    yielder::Yielder,
};
