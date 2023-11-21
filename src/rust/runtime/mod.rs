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
pub mod scheduler;
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
    pal::data_structures::SockAddr,
    runtime::{
        fail::Fail,
        memory::MemoryRuntime,
        network::{
            ephemeral::EphemeralPorts,
            socket::SocketId,
            NetworkQueueTable,
        },
        queue::{
            IoQueue,
            IoQueueTable,
        },
        scheduler::{
            Scheduler,
            Task,
            TaskHandle,
        },
        timer::SharedTimer,
        types::demi_opcode_t,
    },
};
use ::std::{
    boxed::Box,
    future::Future,
    mem,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    rc::Rc,
    time::Instant,
};

#[cfg(target_os = "windows")]
use windows::Win32::Networking::WinSock::{
    WSAEALREADY,
    WSAEINPROGRESS,
    WSAEWOULDBLOCK,
};

#[cfg(target_os = "windows")]
use crate::pal::functions::socketaddrv4_to_sockaddr;

#[cfg(target_os = "linux")]
use crate::pal::linux::socketaddrv4_to_sockaddr;

use self::types::{
    demi_accept_result_t,
    demi_qr_value_t,
    demi_qresult_t,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel Runtime
#[derive(Default)]
pub struct DemiRuntime {
    /// Scheduler
    scheduler: Scheduler,
    /// Shared IoQueueTable.
    qtable: IoQueueTable,
    /// Shared ephemeral port allocator.
    ephemeral_ports: EphemeralPorts,
    /// Shared timer for periodic triggering of coroutines and time outs.
    timer: SharedTimer,
    /// Shared table for mapping from underlying transport identifiers to queue descriptors.
    network_table: NetworkQueueTable,
}

#[derive(Clone)]
pub struct SharedDemiRuntime(SharedObject<DemiRuntime>);

/// The SharedObject wraps an object that will be shared across coroutines.
pub struct SharedObject<T>(Rc<T>);
pub struct SharedBox<T: ?Sized>(SharedObject<Box<T>>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl DemiRuntime {
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

/// Associate Functions for POSIX Runtime
impl SharedDemiRuntime {
    pub fn new(now: Instant) -> Self {
        Self(SharedObject::<DemiRuntime>::new(DemiRuntime {
            scheduler: Scheduler::default(),
            qtable: IoQueueTable::default(),
            ephemeral_ports: EphemeralPorts::default(),
            timer: SharedTimer::new(now),
            network_table: NetworkQueueTable::default(),
        }))
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

    /// Removes a coroutine from the underlying scheduler given its associated [QToken] `qt`.
    pub fn remove_coroutine_with_qtoken(&mut self, qt: QToken) -> OperationTask {
        self.remove_coroutine(&self.scheduler.from_task_id(qt.into()).expect("coroutine should exist"))
    }

    /// Removes a coroutine from the underlying scheduler given its associated [TaskHandle] `handle`
    /// and gets the result immediately.
    pub fn remove_coroutine_and_get_result(&mut self, handle: &TaskHandle, qt: u64) -> demi_qresult_t {
        // 1. Remove Task from scheduler.
        let boxed_task: Box<dyn Task> = self
            .scheduler
            .remove(handle)
            .expect("Removing task that does not exist (either was previously removed or never inserted");
        // 2. Cast to void and then downcast to operation task.
        trace!("Removing coroutine: {:?}", boxed_task.get_name());
        let operation_task: OperationTask = OperationTask::from(boxed_task.as_any());
        let (qd, result) = operation_task.get_result().expect("Coroutine not finished");
        self.pack_result(result, qd, qt)
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
        let qd: QDesc = self.qtable.alloc::<T>(queue);
        trace!("Allocating new queue: qd={:?}", qd);
        qd
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
        trace!("Freeing queue: qd={:?}", qd);
        self.qtable.free(qd)
    }

    /// Gets a reference to a shared queue. It is very important that this function bump the reference count (using
    /// clone) so that we can track how many references to this shared queue that we have handed out.
    /// TODO: This should only return SharedObject types but for now we will also allow other cloneable queue types.
    pub fn get_shared_queue<T: IoQueue + Clone>(&self, qd: &QDesc) -> Result<T, Fail> {
        Ok(self.qtable.get::<T>(qd)?.clone())
    }

    /// Returns the type for the queue that matches [qd].
    pub fn get_queue_type(&self, qd: &QDesc) -> Result<QType, Fail> {
        self.qtable.get_type(qd)
    }

    /// Allocates a port from the shared ephemeral port allocator.
    pub fn alloc_ephemeral_port(&mut self) -> Result<u16, Fail> {
        match self.ephemeral_ports.alloc() {
            Ok(port) => {
                trace!("Allocating ephemeral port: {:?}", port);
                Ok(port)
            },
            Err(e) => {
                warn!("Could not allocate ephemeral port: {:?}", e);
                Err(e)
            },
        }
    }

    /// Reserves a specific port if it is free.
    pub fn reserve_ephemeral_port(&mut self, port: u16) -> Result<(), Fail> {
        match self.ephemeral_ports.reserve(port) {
            Ok(()) => {
                trace!("Reserving ephemeral port: {:?}", port);
                Ok(())
            },
            Err(e) => {
                warn!("Could not reserve ephemeral port: port={:?} error={:?}", port, e);
                Err(e)
            },
        }
    }

    /// Frees an ephemeral port.
    pub fn free_ephemeral_port(&mut self, port: u16) -> Result<(), Fail> {
        match self.ephemeral_ports.free(port) {
            Ok(()) => {
                trace!("Freeing ephemeral port: {:?}", port);
                Ok(())
            },
            Err(e) => {
                warn!("Could not free ephemeral port: port={:?} error={:?}", port, e);
                Err(e)
            },
        }
    }

    /// Checks if a port is private.
    pub fn is_private_ephemeral_port(port: u16) -> bool {
        EphemeralPorts::is_private(port)
    }

    /// Returns a reference to the shared timer.
    pub fn get_timer(&self) -> SharedTimer {
        self.timer.clone()
    }

    /// Moves time forward deterministically.
    pub fn advance_clock(&mut self, now: Instant) {
        self.timer.advance_clock(now)
    }

    /// Gets the current time according to our internal timer.
    pub fn get_now(&self) -> Instant {
        self.timer.now()
    }

    /// Checks if an identifier is in use and returns the queue descriptor if it is.
    pub fn get_qd_from_socket_id(&self, id: &SocketId) -> Option<QDesc> {
        match self.network_table.get_qd(id) {
            Some(qd) => {
                trace!("Looking up queue descriptor: socket_id={:?} qd={:?}", id, qd);
                Some(qd)
            },
            None => {
                trace!("Could not find queue descriptor for socket id: {:?}", id);
                None
            },
        }
    }

    /// Inserts a mapping and returns the previously mapped queue descriptor if it exists.
    pub fn insert_socket_id_to_qd(&mut self, id: SocketId, qd: QDesc) -> Option<QDesc> {
        trace!("Insert socket id to queue descriptor mapping: {:?} -> {:?}", id, qd);
        self.network_table.insert_qd(id, qd)
    }

    /// Removes a mapping and returns the mapped queue descriptor.
    pub fn remove_socket_id_to_qd(&mut self, id: &SocketId) -> Option<QDesc> {
        match self.network_table.remove_qd(id) {
            Some(qd) => {
                trace!("Remove socket id to queue descriptor mapping: {:?} -> {:?}", id, qd);
                Some(qd)
            },
            None => {
                trace!(
                    "Remove but could not find socket id to queue descriptor mapping: {:?}",
                    id
                );
                None
            },
        }
    }

    pub fn addr_in_use(&self, local: SocketAddrV4) -> bool {
        trace!("Check address in use: {:?}", local);
        self.network_table.addr_in_use(local)
    }

    pub fn pack_result(&self, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
        match result {
            OperationResult::Connect => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Accept((new_qd, addr)) => {
                let saddr: SockAddr = socketaddrv4_to_sockaddr(&addr);
                let qr_value: demi_qr_value_t = demi_qr_value_t {
                    ares: demi_accept_result_t {
                        qd: new_qd.into(),
                        addr: saddr,
                    },
                };
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_ACCEPT,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_ret: 0,
                    qr_value,
                }
            },
            OperationResult::Push => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Pop(addr, bytes) => match self.into_sgarray(bytes) {
                Ok(mut sga) => {
                    if let Some(addr) = addr {
                        sga.sga_addr = socketaddrv4_to_sockaddr(&addr);
                    }
                    let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                    demi_qresult_t {
                        qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                        qr_qd: qd.into(),
                        qr_qt: qt,
                        qr_ret: 0,
                        qr_value,
                    }
                },
                Err(e) => {
                    warn!("Operation Failed: {:?}", e);
                    demi_qresult_t {
                        qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                        qr_qd: qd.into(),
                        qr_qt: qt,
                        qr_ret: e.errno as i64,
                        qr_value: unsafe { mem::zeroed() },
                    }
                },
            },
            OperationResult::Close => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_CLOSE,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Failed(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_ret: e.errno as i64,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        }
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
impl MemoryRuntime for SharedDemiRuntime {}

impl Default for SharedDemiRuntime {
    fn default() -> Self {
        Self(SharedObject::new(DemiRuntime::default()))
    }
}

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
