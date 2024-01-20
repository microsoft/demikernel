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
            TaskId,
        },
        timer::SharedTimer,
        types::demi_opcode_t,
    },
};
use ::futures::future::FusedFuture;
use ::std::{
    boxed::Box,
    collections::HashMap,
    convert::{
        AsMut,
        AsRef,
    },
    mem,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    pin::Pin,
    rc::Rc,
    time::{
        Duration,
        Instant,
        SystemTime,
    },
};

#[cfg(target_os = "windows")]
use crate::pal::functions::socketaddrv4_to_sockaddr;

#[cfg(target_os = "linux")]
use crate::pal::linux::socketaddrv4_to_sockaddr;

use self::{
    scheduler::{
        Yielder,
        YielderHandle,
    },
    types::{
        demi_accept_result_t,
        demi_qr_value_t,
        demi_qresult_t,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

const TIMER_RESOLUTION: usize = 64;

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
    /// Currently running coroutines.
    pending_ops: HashMap<QDesc, HashMap<QToken, YielderHandle>>,
    /// Number of iterations that we have polled since advancing the clock.
    ts_iters: usize,
    /// Tasks that have been completed and removed from the
    completed_tasks: HashMap<QToken, (QDesc, OperationResult)>,
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
        if errno == libc::EINPROGRESS || errno == libc::EWOULDBLOCK || errno == libc::EAGAIN || errno == libc::EALREADY
        {
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
            pending_ops: HashMap::<QDesc, HashMap<QToken, YielderHandle>>::new(),
            ts_iters: 0,
            completed_tasks: HashMap::<QToken, (QDesc, OperationResult)>::new(),
        }))
    }

    /// Inserts the `coroutine` named `task_name` into the scheduler.
    fn insert_coroutine(&mut self, task_name: &str, coroutine: Pin<Box<Operation>>) -> Result<QToken, Fail> {
        trace!("Inserting coroutine: {:?}", task_name);
        let task: OperationTask = OperationTask::new(task_name.to_string(), coroutine);
        match self.scheduler.insert_task(task) {
            Some(task_id) => Ok(task_id.into()),
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
        }
    }

    /// The coroutine factory is a function that takes a yielder and returns a future. The future is then inserted into
    /// the scheduler.
    pub fn insert_coroutine_with_tracking<F>(
        &mut self,
        task_name: &str,
        coroutine_factory: F,
        qd: QDesc,
    ) -> Result<QToken, Fail>
    where
        F: FnOnce(Yielder) -> Pin<Box<dyn FusedFuture<Output = (QDesc, OperationResult)>>>,
    {
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let coroutine: Pin<Box<dyn FusedFuture<Output = (QDesc, OperationResult)>>> = coroutine_factory(yielder);
        match self.insert_coroutine(task_name, coroutine) {
            Ok(qt) => {
                // This allows to keep track of currently running coroutines.
                self.pending_ops
                    .entry(qd)
                    .or_insert(HashMap::new())
                    .insert(qt, yielder_handle.clone());
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Duration) -> Result<demi_qresult_t, Fail> {
        trace!("wait(): qt={:?}, timeout={:?}", qt, timeout);

        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];

        // Call wait_any() to do the real work.
        let (offset, qr): (usize, demi_qresult_t) = self.wait_any(&qt_array, timeout)?;
        debug_assert_eq!(offset, 0);
        Ok(qr)
    }

    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        if let Some((qd, result)) = self.completed_tasks.remove(&qt) {
            let result: demi_qresult_t = self.create_result(result, qd, qt);
            return Ok(result);
        }
        if !self.scheduler.is_valid_task(&TaskId::from(qt)) {
            let cause: String = format!("{:?} is not a valid queue token", qt);
            warn!("wait_any: {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // 2. None of the tasks have already completed, so start a timer and move the clock.
        let start: Instant = self.get_now();
        self.advance_clock(start);

        loop {
            if let Some(boxed_task) = self.scheduler.get_next_completed_task(TIMER_RESOLUTION) {
                // Perform bookkeeping for the completed and removed task.
                trace!("Removing coroutine: {:?}", boxed_task.get_name());
                let completed_qt: QToken = boxed_task.get_id().into();
                // If an operation task (and not a background task), then check the task to see if it is one of ours.
                if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                    let (qd, result): (QDesc, OperationResult) =
                        operation_task.get_result().expect("coroutine not finished");
                    self.cancel_or_remove_pending_ops_as_needed(&result, qd, completed_qt);

                    // Check whether it matches any of the queue tokens that we are waiting on.
                    if completed_qt == qt {
                        let result: demi_qresult_t = self.create_result(result, qd, qt);
                        return Ok(result);
                    }

                    // If not a queue token that we are waiting on, then insert into our list of completed tasks.
                    self.completed_tasks.insert(qt, (qd, result));
                }
            }
            // Check the timeout.
            if let Some(abstime) = abstime {
                if SystemTime::now() >= abstime {
                    return Err(Fail::new(libc::ETIMEDOUT, "wait timed out"));
                }
            }

            // Advance the clock and continue running tasks.
            let now: Instant = self.get_now();
            self.advance_clock(now);
        }
    }

    /// Waits until one of the tasks in qts has completed and returns the result.
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Duration) -> Result<(usize, demi_qresult_t), Fail> {
        for (i, qt) in qts.iter().enumerate() {
            // 1. Check if any of these queue tokens point to already completed tasks.
            if let Some((qd, result)) = self.get_completed_task(&qt) {
                return Ok((i, self.create_result(result, qd, *qt)));
            }

            // 2. Make sure these queue tokens all point to valid tasks.
            if !self.scheduler.is_valid_task(&TaskId::from(*qt)) {
                let cause: String = format!("{:?} is not a valid queue token", qt);
                warn!("wait_any: {}", cause);
                return Err(Fail::new(libc::EINVAL, &cause));
            }
        }

        // 3. None of the tasks have already completed, so start a timer and move the clock.
        let start: Instant = self.get_now();
        self.advance_clock(start);

        // 4. Invoke the scheduler and run some tasks.
        loop {
            // Run for one quanta and if one of our queue tokens completed, then return.
            if let Some((i, qd, result)) = self.run_any(qts) {
                return Ok((i, self.create_result(result, qd, qts[i])));
            }
            // Otherwise, move time forward.
            let now: Instant = self.get_now();
            if now >= start + timeout {
                return Err(Fail::new(libc::ETIMEDOUT, "wait timed out"));
            } else {
                self.advance_clock(now);
            }
        }
    }

    pub fn get_completed_task(&mut self, qt: &QToken) -> Option<(QDesc, OperationResult)> {
        self.completed_tasks.remove(qt)
    }

    /// Runs the scheduler for one [TIMER_RESOLUTION] quanta. Importantly does not modify the clock.
    pub fn run_any(&mut self, qts: &[QToken]) -> Option<(usize, QDesc, OperationResult)> {
        if let Some(boxed_task) = self.scheduler.get_next_completed_task(TIMER_RESOLUTION) {
            // Perform bookkeeping for the completed and removed task.
            trace!("Removing coroutine: {:?}", boxed_task.get_name());
            let qt: QToken = boxed_task.get_id().into();

            // If an operation task, then take a look at the result.
            if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                let (qd, result): (QDesc, OperationResult) =
                    operation_task.get_result().expect("coroutine not finished");
                self.cancel_or_remove_pending_ops_as_needed(&result, qd, qt);

                // Check whether it matches any of the queue tokens that we are waiting on.
                for i in 0..qts.len() {
                    if qts[i] == qt {
                        return Some((i, qd, result));
                    }
                }

                // If not a queue token that we are waiting on, then insert into our list of completed tasks.
                self.completed_tasks.insert(qt, (qd, result));
            }
        }
        None
    }

    /// When the queue is closed, we need to cancel all pending ops. When the coroutine is removed, we only need to
    /// cancel the pending op associated with the qtoken.
    fn cancel_or_remove_pending_ops_as_needed(&mut self, result: &OperationResult, qd: QDesc, qt: QToken) {
        match result {
            OperationResult::Close => {
                self.cancel_all_pending_ops_for_queue(&qd);
            },
            _ => {
                self.cancel_pending_op(&qd, &qt);
            },
        }
    }

    /// Cancel pending op because the coroutine was removed.
    fn cancel_pending_op(&mut self, qd: &QDesc, qt: &QToken) {
        if let Some(inner_hash_map) = self.pending_ops.get_mut(&qd) {
            inner_hash_map.remove(qt);
        }
    }

    /// Cancel all pending ops because the queue was closed.
    fn cancel_all_pending_ops_for_queue(&mut self, qd: &QDesc) {
        if let Some(inner_hash_map) = &mut self.pending_ops.remove(&qd) {
            let drain = inner_hash_map.drain();
            for (_, mut yielder_handle) in drain {
                yielder_handle.wake_with(Err(Fail::new(libc::ECANCELED, "This queue was closed")));
            }
        }
    }

    /// Inserts the background `coroutine` named `task_name` into the scheduler.
    pub fn insert_background_coroutine(
        &mut self,
        task_name: &str,
        coroutine: Pin<Box<dyn FusedFuture<Output = ()>>>,
    ) -> Result<QToken, Fail> {
        trace!("Inserting background coroutine: {:?}", task_name);
        let task: BackgroundTask = BackgroundTask::new(task_name.to_string(), coroutine);
        match self.scheduler.insert_task(task) {
            Some(task_id) => Ok(task_id.into()),
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_background_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
        }
    }

    /// Removes the background `coroutine` associated with `qt`. Since background coroutines do not return a result
    /// there is no need to cast it.
    pub fn remove_background_coroutine(&mut self, qt: QToken) -> Result<(), Fail> {
        match self.scheduler.remove_task(qt.into()) {
            Some(boxed_task) => {
                trace!("Removing background coroutine: {:?}", boxed_task.get_name());
                Ok(())
            },
            None => {
                let cause: String = format!("cannot remove coroutine (task_id={:?})", qt);
                error!("remove_background_coroutine(): {}", cause);
                Err(Fail::new(libc::ESRCH, &cause))
            },
        }
    }

    pub fn poll_and_advance_clock(&mut self) {
        if self.ts_iters == 0 {
            self.advance_clock(Instant::now());
        }
        self.ts_iters = (self.ts_iters + 1) % TIMER_RESOLUTION;
        self.poll()
    }

    /// Performs a single pool on the underlying scheduler.
    pub fn poll(&mut self) {
        // For all ready tasks that were removed from the scheduler, add to our completed task list.
        for boxed_task in self.scheduler.poll_all() {
            trace!("Completed while polling coroutine: {:?}", boxed_task.get_name());
            let qt: QToken = boxed_task.get_id().into();

            if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                let (qd, result): (QDesc, OperationResult) =
                    operation_task.get_result().expect("coroutine not finished");
                self.cancel_or_remove_pending_ops_as_needed(&result, qd, qt);
                self.completed_tasks.insert(qt, (qd, result));
            }
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
        self.cancel_all_pending_ops_for_queue(qd);
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

    pub fn create_result(&self, result: OperationResult, qd: QDesc, qt: QToken) -> demi_qresult_t {
        match result {
            OperationResult::Connect => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
                qr_qd: qd.into(),
                qr_qt: qt.into(),
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
                    qr_qt: qt.into(),
                    qr_ret: 0,
                    qr_value,
                }
            },
            OperationResult::Push => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
                qr_qd: qd.into(),
                qr_qt: qt.into(),
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
                        qr_qt: qt.into(),
                        qr_ret: 0,
                        qr_value,
                    }
                },
                Err(e) => {
                    warn!("Operation Failed: {:?}", e);
                    demi_qresult_t {
                        qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                        qr_qd: qd.into(),
                        qr_qt: qt.into(),
                        qr_ret: e.errno as i64,
                        qr_value: unsafe { mem::zeroed() },
                    }
                },
            },
            OperationResult::Close => demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_CLOSE,
                qr_qd: qd.into(),
                qr_qt: qt.into(),
                qr_ret: 0,
                qr_value: unsafe { mem::zeroed() },
            },
            OperationResult::Failed(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt.into(),
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

/// Dereferences a shared object for use.
impl<T> Deref for SharedObject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// Dereferences a mutable reference to a shared object for use. This breaks Rust's ownership model because it allows
/// more than one mutable dereference of a shared object at a time. Demikernel requires this because multiple
/// coroutines will have mutable references to shared objects at the same time; however, Demikernel also ensures that
/// only one coroutine will run at a time. Due to this design, Rust's static borrow checker is not able to ensure
/// memory safety and we have chosen not to use the dynamic borrow checker. Instead, shared objects should be used
/// judiciously across coroutines with the understanding that the shared object may change/be mutated whenever the
/// coroutine yields.
impl<T> DerefMut for SharedObject<T> {
    fn deref_mut<'a>(&'a mut self) -> &'a mut Self::Target {
        let ptr: *mut T = Rc::as_ptr(&self.0) as *mut T;
        unsafe { &mut *ptr }
    }
}

/// Returns a reference to the interior object, which is borrowed for directly accessing the value. Generally deref
/// should be used unless you absolutely need to borrow the reference.
impl<T> AsRef<T> for SharedObject<T> {
    fn as_ref(&self) -> &T {
        self.0.as_ref()
    }
}

/// Returns a mutable reference to the interior object. Similar to DerefMut, this breaks Rust's ownership properties
/// and should be considered unsafe. However, it is safe to use in Demikernel if and only if we only run one coroutine
/// at a time.
impl<T> AsMut<T> for SharedObject<T> {
    fn as_mut<'a>(&'a mut self) -> &'a mut T {
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
