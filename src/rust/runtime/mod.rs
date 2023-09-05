// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod fail;
pub mod limits;
pub mod logging;
pub mod memory;
pub mod network;
pub mod queue;
pub mod timer;
pub mod types;
pub mod watched;
pub use queue::{
    BackgroundTask,
    Operation,
    OperationResult,
    OperationTask,
    QDesc,
    QToken,
    QType,
};

#[cfg(feature = "liburing")]
pub use liburing;

#[cfg(feature = "libdpdk")]
pub use dpdk_rs as libdpdk;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        queue::{
            IoQueue,
            IoQueueTable,
        },
    },
    scheduler::{
        scheduler::Scheduler,
        Task,
        TaskHandle,
    },
};
use ::std::{
    boxed::Box,
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    pin::Pin,
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel Runtime
#[derive(Clone)]
pub struct DemiRuntime {
    /// Scheduler
    scheduler: Scheduler,
    /// Shared IoQueueTable.
    qtable: Rc<RefCell<IoQueueTable>>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for POSIX Runtime
impl DemiRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
            qtable: Rc::new(RefCell::<IoQueueTable>::new(IoQueueTable::new())),
        }
    }

    /// Inserts the `coroutine` named `task_name` into the scheduler.
    pub fn insert_coroutine(&self, task_name: &str, coroutine: Pin<Box<Operation>>) -> Result<TaskHandle, Fail> {
        let task: OperationTask = OperationTask::new(task_name.to_string(), coroutine);
        match self.scheduler.insert(task) {
            Some(handle) => Ok(handle),
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
        }
    }

    /// Removes a coroutine from the underlying scheduler given its associated [TaskHandle] `handle`.
    pub fn remove_coroutine(&self, handle: &TaskHandle) -> OperationTask {
        // 1. Remove Task from scheduler.
        let boxed_task: Box<dyn Task> = self
            .scheduler
            .remove(handle)
            .expect("Removing task that does not exist (either was previously removed or never inserted");
        // 2. Cast to void and then downcast to operation task.
        OperationTask::from(boxed_task.as_any())
    }

    /// Performs a single pool on the underlying scheduler.
    pub fn poll(&self) {
        self.scheduler.poll()
    }

    /// Retrieves the [TaskHandle] associated with the given [QToken] `qt`.
    pub fn from_task_id(&self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self.scheduler.from_task_id(qt.into()) {
            Some(handle) => Ok(handle),
            None => {
                let cause: String = format!("invalid queue token (qt={:?})", &qt);
                error!("from_task_id(): {}", cause);
                Err(Fail::new(libc::EINVAL, &cause))
            },
        }
    }

    /// Allocates a queue of type `T` and returns the associated queue descriptor.
    pub fn alloc_queue<T: IoQueue>(&self, queue: T) -> QDesc {
        self.qtable.borrow_mut().alloc::<T>(queue)
    }

    /// Returns a reference to the I/O queue table.
    pub fn get_qtable(&self) -> Ref<IoQueueTable> {
        self.qtable.borrow()
    }

    /// Returns a mutable reference to the I/O queue table.
    pub fn get_mut_qtable(&self) -> RefMut<IoQueueTable> {
        self.qtable.borrow_mut()
    }

    /// Frees the queue associated with [qd] and returns the freed queue.
    pub fn free_queue<T: IoQueue>(&self, qd: &QDesc) -> Result<T, Fail> {
        self.qtable.borrow_mut().free(qd)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for DemiRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for DemiRuntime {}

//======================================================================================================================
// Traits
//======================================================================================================================

/// Demikernel Runtime
pub trait Runtime: Clone + Unpin + 'static {}
