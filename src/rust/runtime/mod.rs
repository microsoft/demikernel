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
    future::Future,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    rc::Rc,
};

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::{
    WSAEALREADY,
    WSAEINPROGRESS,
    WSAEWOULDBLOCK,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel Runtime
pub struct DemiRuntime {
    /// Scheduler
    scheduler: Scheduler,
    /// Shared IoQueueTable.
    qtable: IoQueueTable,
}

#[derive(Clone)]
pub struct SharedDemiRuntime(SharedObject<DemiRuntime>);

/// The SharedObject wraps an object that will be shared across coroutines.
pub struct SharedObject<T>(Rc<T>);
pub struct SharedBox<T: ?Sized>(SharedObject<Box<T>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl SharedDemiRuntime {
    pub fn new() -> Self {
        Self(SharedObject::new(DemiRuntime::new()))
    }
}

/// Associate Functions for POSIX Runtime
impl DemiRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
            qtable: IoQueueTable::new(),
        }
    }

    /// Inserts the `coroutine` named `task_name` into the scheduler.
    pub fn insert_coroutine(&mut self, task_name: &str, coroutine: Pin<Box<Operation>>) -> Result<TaskHandle, Fail> {
        trace!("Inserting coroutine: {:?}", task_name);
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
    pub fn remove_coroutine(&mut self, handle: &TaskHandle) -> OperationTask {
        // 1. Remove Task from scheduler.
        let boxed_task: Box<dyn Task> = self
            .scheduler
            .remove(handle)
            .expect("Removing task that does not exist (either was previously removed or never inserted");
        // 2. Cast to void and then downcast to operation task.
        trace!("Removing coroutine: {:?}", boxed_task.get_name());
        OperationTask::from(boxed_task.as_any())
    }

    /// Inserts the background `coroutine` named `task_name` into the scheduler.
    pub fn insert_background_coroutine(
        &mut self,
        task_name: &str,
        coroutine: Pin<Box<dyn Future<Output = ()>>>,
    ) -> Result<TaskHandle, Fail> {
        trace!("Inserting background coroutine: {:?}", task_name);
        let task: BackgroundTask = BackgroundTask::new(task_name.to_string(), coroutine);
        match self.scheduler.insert(task) {
            Some(handle) => Ok(handle),
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_background_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
        }
    }

    /// Removes the background `coroutine` associated with `handle`. Since background coroutines do not return a result
    /// there is no need to cast it.
    pub fn remove_background_coroutine(&mut self, handle: &TaskHandle) -> Result<(), Fail> {
        match self.scheduler.remove(handle) {
            Some(boxed_task) => {
                trace!("Removing background coroutine: {:?}", boxed_task.get_name());
                Ok(())
            },
            None => {
                let cause: String = format!("cannot remove coroutine (task_id={:?})", &handle.get_task_id());
                error!("remove_background_coroutine(): {}", cause);
                Err(Fail::new(libc::ESRCH, &cause))
            },
        }
    }

    /// Performs a single pool on the underlying scheduler.
    pub fn poll(&mut self) {
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
    pub fn alloc_queue<T: IoQueue>(&mut self, queue: T) -> QDesc {
        self.qtable.alloc::<T>(queue)
    }

    /// Returns a reference to the I/O queue table.
    pub fn get_qtable(&self) -> &IoQueueTable {
        &self.qtable
    }

    /// Returns a mutable reference to the I/O queue table.
    pub fn get_mut_qtable(&mut self) -> &mut IoQueueTable {
        &mut self.qtable
    }

    /// Frees the queue associated with [qd] and returns the freed queue.
    pub fn free_queue<T: IoQueue>(&mut self, qd: &QDesc) -> Result<T, Fail> {
        self.qtable.free(qd)
    }

    /// Gets a reference to a shared queue. It is very important that this function bump the reference count (using
    /// clone) so that we can track how many references to this shared queue that we have handed out.
    /// TODO: This should only return SharedObject types but for now we will also allow other cloneable queue types.
    pub fn get_shared_queue<T: IoQueue + Clone>(&self, qd: &QDesc) -> Result<T, Fail> {
        Ok(self.qtable.get::<T>(qd)?.clone())
    }

    pub fn get_queue_type(&self, qd: &QDesc) -> Result<QType, Fail> {
        self.qtable.get_type(qd)
    }

    /// Checks if an operation should be retried based on the error code `err`.
    pub fn should_retry(errno: i32) -> bool {
        #[cfg(target_os = "linux")]
        if errno == libc::EINPROGRESS || errno == libc::EWOULDBLOCK || errno == libc::EAGAIN || errno == libc::EALREADY
        {
            return true;
        }

        #[cfg(target_os = "windows")]
        if errno == WSAEWOULDBLOCK.0 || errno == WSAEINPROGRESS.0 || errno == WSAEALREADY.0 {
            return true;
        }

        false
    }
}

impl<T> SharedObject<T> {
    pub fn new(object: T) -> Self {
        Self(Rc::new(object))
    }
}

impl<T: ?Sized> SharedBox<T> {
    pub fn new(boxed_object: Box<T>) -> Self {
        Self(SharedObject::<Box<T>>::new(boxed_object))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for DemiRuntime {}

impl<T> Deref for SharedObject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T> DerefMut for SharedObject<T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut Self::Target {
        let ptr: *mut T = Rc::as_ptr(&self.0) as *mut T;
        unsafe { &mut *ptr }
    }
}

impl<T> Clone for SharedObject<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> Deref for SharedBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: ?Sized> DerefMut for SharedBox<T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut Self::Target {
        self.0.deref_mut().as_mut()
    }
}

impl<T: ?Sized> Clone for SharedBox<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl Deref for SharedDemiRuntime {
    type Target = DemiRuntime;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedDemiRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

//======================================================================================================================
// Traits
//======================================================================================================================

/// Demikernel Runtime
pub trait Runtime: Clone + Unpin + 'static {}
