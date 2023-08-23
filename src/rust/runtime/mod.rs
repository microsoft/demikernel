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
            NetworkQueue,
        },
    },
    scheduler::{
        scheduler::Scheduler,
        TaskHandle,
    },
};
use ::std::{
    any::{
        Any,
        TypeId,
    },
    boxed::Box,
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    net::SocketAddrV4,
    pin::Pin,
    rc::Rc,
};

type IoQueueType = Box<dyn NetworkQueue>;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel Runtime
#[derive(Clone)]
pub struct DemiRuntime {
    /// Scheduler
    scheduler: Scheduler,
    /// Shared IoQueueTable.
    /// FIXME: Currently only holds network queues. Change to IoQueue once all libOSes are merged.
    qtable: Rc<RefCell<IoQueueTable<IoQueueType>>>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for POSIX Runtime
impl DemiRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
            qtable: Rc::new(RefCell::<IoQueueTable<IoQueueType>>::new(
                IoQueueTable::<IoQueueType>::new(),
            )),
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
        OperationTask::from(
            self.scheduler
                .remove(&handle)
                .expect("Removing task that does not exist (either was previously removed or never inserted")
                .as_any(),
        )
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

    pub fn alloc_queue<T: NetworkQueue>(&self, queue: T) -> QDesc {
        self.qtable.borrow_mut().alloc(Box::new(queue))
    }

    pub fn get_qtable(&self) -> Ref<IoQueueTable<Box<dyn NetworkQueue>>> {
        self.qtable.borrow()
    }

    pub fn get_mut_qtable(&self) -> RefMut<IoQueueTable<Box<dyn NetworkQueue>>> {
        self.qtable.borrow_mut()
    }

    pub fn free_queue<T: NetworkQueue + Clone>(&self, qd: QDesc) -> Option<T> {
        match self.qtable.borrow_mut().free(&qd) {
            Some(queue) => {
                let queue_ptr: &dyn Any = queue.as_any_ref();
                if queue_ptr.type_id() == TypeId::of::<T>() {
                    Some(
                        queue_ptr
                            .downcast_ref::<T>()
                            .expect("We have already checked the queue type first")
                            .clone(),
                    )
                } else {
                    None
                }
            },
            None => None,
        }
    }

    pub fn get_queue<T: NetworkQueue + Clone>(&self, qd: QDesc) -> Result<T, Fail> {
        // Hack to clone the queue.
        match self.qtable.borrow().get(&qd) {
            Some(queue) => {
                let queue_ptr: &dyn Any = queue.as_ref().as_any_ref();
                match queue_ptr.downcast_ref::<T>() {
                    Some(result) => Ok(result.clone()),
                    None => Err(Fail::new(libc::EBADF, "invalid queue descriptor type")),
                }
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for DemiRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for DemiRuntime {}

/// IoQueue trait for IoQueueType

/// NetworkQueue trait for IoQueueType
impl IoQueue for IoQueueType {
    fn get_qtype(&self) -> QType {
        self.as_ref().get_qtype()
    }
}

impl NetworkQueue for IoQueueType {
    /// Returns the local address to which the target queue is bound.
    fn local(&self) -> Option<SocketAddrV4> {
        self.as_ref().local()
    }

    /// Returns the remote address to which the target queue is connected to.
    fn remote(&self) -> Option<SocketAddrV4> {
        self.as_ref().remote()
    }

    /// Returns an Any reference that can be cast to the actual queue.
    fn as_any_ref(&self) -> &dyn Any {
        self
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

/// Demikernel Runtime
pub trait Runtime: Clone + Unpin + 'static {}
