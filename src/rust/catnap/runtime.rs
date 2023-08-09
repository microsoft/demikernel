// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        Operation,
        OperationTask,
        QToken,
        Runtime,
    },
    scheduler::{
        scheduler::Scheduler,
        TaskHandle,
    },
};
use ::std::{
    boxed::Box,
    pin::Pin,
};

//==============================================================================
// Structures
//==============================================================================

/// POSIX Runtime
#[derive(Clone)]
pub struct PosixRuntime {
    /// Scheduler
    pub scheduler: Scheduler,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for POSIX Runtime
impl PosixRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
        }
    }

    /// This function inserts a given [coroutine] into the scheduler with [task_id] to identify it.
    pub fn insert_coroutine(&self, task_id: String, coroutine: Pin<Box<Operation>>) -> Result<TaskHandle, Fail> {
        let task: OperationTask = OperationTask::new(task_id, coroutine);
        match self.scheduler.insert(task) {
            Some(handle) => Ok(handle),
            None => Err(Fail::new(libc::EAGAIN, "cannot schedule coroutine")),
        }
    }

    /// This function removes a coroutine from the scheduler given a task [handle].
    pub fn remove_coroutine(&self, handle: &TaskHandle) -> OperationTask {
        OperationTask::from(
            self.scheduler
                .remove(&handle)
                .expect("Removing task that does not exist (either was previously removed or never inserted")
                .as_any(),
        )
    }

    /// This function polls the scheduler for one iteration.
    pub fn poll(&self) {
        self.scheduler.poll()
    }

    /// Turn a [qt] QToken into a [TaskHandle].
    pub fn from_task_id(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for PosixRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for PosixRuntime {}
