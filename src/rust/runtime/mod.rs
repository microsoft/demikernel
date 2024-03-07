// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod condition_variable;
pub mod fail;
pub mod limits;
pub mod logging;
pub mod memory;
pub mod network;
pub mod queue;
pub mod scheduler;
pub mod types;
pub use condition_variable::SharedConditionVariable;
mod poll;
mod timer;
pub use queue::{
    BackgroundTask,
    Operation,
    OperationResult,
    OperationTask,
    QDesc,
    QToken,
    QType,
};
pub use scheduler::TaskId;

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
        poll::PollFuture,
        queue::{
            IoQueue,
            IoQueueTable,
        },
        scheduler::{
            SharedScheduler,
            TaskWithResult,
        },
        timer::SharedTimer,
        types::{
            demi_accept_result_t,
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
        },
    },
};
use ::futures::{
    future::FusedFuture,
    select_biased,
    Future,
    FutureExt,
};

use ::std::{
    any::Any,
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
    pin::{
        pin,
        Pin,
    },
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

//======================================================================================================================
// Thread local variable
//======================================================================================================================

thread_local! {
/// Each Demikernel thread has its own instance of the scheduler stored in a thread local variable for access from
/// different coroutines. It is important to note that this is NEVER accessed directly from outside of the runtime.
static THREAD_SCHEDULER: SharedScheduler = SharedScheduler::default();
/// This is our shared sense of time. It is explicitly moved forward ONLY by the runtime and used to trigger time outs.
static THREAD_TIME: SharedTimer = SharedTimer::default();
}

//======================================================================================================================
// Constants
//======================================================================================================================

const TIMER_RESOLUTION: usize = 64;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Demikernel Runtime
pub struct DemiRuntime {
    /// Shared IoQueueTable.
    qtable: IoQueueTable,
    /// Shared ephemeral port allocator.
    ephemeral_ports: EphemeralPorts,
    /// Shared table for mapping from underlying transport identifiers to queue descriptors.
    network_table: NetworkQueueTable,
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
    #[cfg(test)]
    pub fn new(now: Instant) -> Self {
        THREAD_TIME.with(|s| {
            s.clone().set_time(now);
        });

        Self(SharedObject::<DemiRuntime>::new(DemiRuntime {
            qtable: IoQueueTable::default(),
            ephemeral_ports: EphemeralPorts::default(),
            network_table: NetworkQueueTable::default(),
            ts_iters: 0,
            completed_tasks: HashMap::<QToken, (QDesc, OperationResult)>::new(),
        }))
    }

    /// Inserts the `coroutine` named `task_name` into the scheduler.
    pub fn insert_io_coroutine(&mut self, task_name: &str, coroutine: Pin<Box<Operation>>) -> Result<QToken, Fail> {
        self.insert_coroutine(task_name, coroutine)
    }

    /// Inserts the background `coroutine` named `task_name` into the scheduler
    pub fn insert_background_coroutine(
        &mut self,
        task_name: &str,
        coroutine: Pin<Box<dyn FusedFuture<Output = ()>>>,
    ) -> Result<QToken, Fail> {
        self.insert_coroutine(task_name, coroutine)
    }

    /// Inserts a coroutine of type T and task
    pub fn insert_coroutine<R: Unpin + Clone + Any>(
        &mut self,
        task_name: &str,
        coroutine: Pin<Box<dyn FusedFuture<Output = R>>>,
    ) -> Result<QToken, Fail> {
        trace!("Inserting coroutine: {:?}", task_name);
        let task: TaskWithResult<R> = TaskWithResult::<R>::new(task_name.to_string(), coroutine);
        match THREAD_SCHEDULER.with(|s| s.clone().insert_task(task)) {
            Some(task_id) => Ok(task_id.into()),
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_background_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
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
        if !THREAD_SCHEDULER.with(|s| s.is_valid_task(&TaskId::from(qt))) {
            let cause: String = format!("{:?} is not a valid queue token", qt);
            warn!("wait_any: {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // 2. None of the tasks have already completed, so start a timer and move the clock.
        self.advance_clock_to_now();

        loop {
            if let Some(boxed_task) = THREAD_SCHEDULER.with(|s| s.clone().get_next_completed_task(TIMER_RESOLUTION)) {
                // Perform bookkeeping for the completed and removed task.
                trace!("Removing coroutine: {:?}", boxed_task.get_name());
                let completed_qt: QToken = boxed_task.get_id().into();
                // If an operation task (and not a background task), then check the task to see if it is one of ours.
                if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                    let (qd, result): (QDesc, OperationResult) =
                        operation_task.get_result().expect("coroutine not finished");

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
            self.advance_clock_to_now();
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
            if !THREAD_SCHEDULER.with(|s| s.is_valid_task(&TaskId::from(*qt))) {
                let cause: String = format!("{:?} is not a valid queue token", qt);
                warn!("wait_any: {}", cause);
                return Err(Fail::new(libc::EINVAL, &cause));
            }
        }

        // 3. None of the tasks have already completed, so start a timer and move the clock.
        self.advance_clock_to_now();
        let start: Instant = self.get_now();

        // 4. Invoke the scheduler and run some tasks.
        loop {
            // Run for one quanta and if one of our queue tokens completed, then return.
            if let Some((i, qd, result)) = self.run_any(qts) {
                return Ok((i, self.create_result(result, qd, qts[i])));
            }
            // Otherwise, move time forward.
            self.advance_clock_to_now();
            let now: Instant = self.get_now();
            if now >= start + timeout {
                return Err(Fail::new(libc::ETIMEDOUT, "wait timed out"));
            }
        }
    }

    pub fn get_completed_task(&mut self, qt: &QToken) -> Option<(QDesc, OperationResult)> {
        self.completed_tasks.remove(qt)
    }

    /// Runs the scheduler for one [TIMER_RESOLUTION] quanta. Importantly does not modify the clock.
    pub fn run_any(&mut self, qts: &[QToken]) -> Option<(usize, QDesc, OperationResult)> {
        if let Some(boxed_task) = THREAD_SCHEDULER.with(|s| s.clone().get_next_completed_task(TIMER_RESOLUTION)) {
            // Perform bookkeeping for the completed and removed task.
            trace!("Removing coroutine: {:?}", boxed_task.get_name());
            let qt: QToken = boxed_task.get_id().into();

            // If an operation task, then take a look at the result.
            if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                let (qd, result): (QDesc, OperationResult) =
                    operation_task.get_result().expect("coroutine not finished");

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

    /// Performs a single pool on the underlying scheduler.
    pub fn poll(&mut self) {
        // For all ready tasks that were removed from the scheduler, add to our completed task list.
        for boxed_task in THREAD_SCHEDULER.with(|s| s.clone().poll_all()) {
            trace!("Completed while polling coroutine: {:?}", boxed_task.get_name());
            let qt: QToken = boxed_task.get_id().into();

            if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                let (qd, result): (QDesc, OperationResult) =
                    operation_task.get_result().expect("coroutine not finished");
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

    /// Moves time forward deterministically.
    pub fn advance_clock(&mut self, now: Instant) {
        THREAD_TIME.with(|t| t.clone().advance_clock(now));
    }

    /// Moves time forward to the current real time.
    fn advance_clock_to_now(&mut self) {
        if self.ts_iters == 0 {
            self.advance_clock(Instant::now());
        }
        self.ts_iters = (self.ts_iters + 1) % TIMER_RESOLUTION;
    }

    /// Gets the current time according to our internal timer.
    pub fn get_now(&self) -> Instant {
        THREAD_TIME.with(|t| t.now())
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
// Static Functions
//======================================================================================================================

/// Check whether this task (and therefore its waker) is still valid.
fn is_valid_task_id(task_id: &TaskId) -> bool {
    THREAD_SCHEDULER.with(|s| s.is_valid_task(task_id))
}

pub async fn yield_with_timeout(timeout: Duration) {
    let timer_cond: SharedConditionVariable = SharedConditionVariable::default();
    THREAD_TIME.with(|t| t.clone().wait(timeout, timer_cond.clone())).await
}

/// Yield until either the condition completes or we time out. If the timeout is 0, then run
pub async fn conditional_yield_with_timeout<F: Future>(condition: F, timeout: Duration) -> Result<F::Output, Fail> {
    let mut timer_cond: SharedConditionVariable = SharedConditionVariable::default();
    select_biased! {
        result = pin!(condition.fuse()) => {
            timer_cond.cancel();
            Ok(result)},
        _ = THREAD_TIME.with(|t| {t.clone().wait(timeout, timer_cond.clone()).fuse()}) => Err(Fail::new(libc::ETIMEDOUT, "a conditional wait timed out"))
    }
}

/// Yield until either the condition completes or the [expiry] time passes. If the expiry time is None, then wait until
/// the condition completes.
pub async fn conditional_yield_until<F: Future>(condition: F, expiry: Option<Instant>) -> Result<F::Output, Fail> {
    if let Some(expiry) = expiry {
        let mut timer_cond: SharedConditionVariable = SharedConditionVariable::default();
        select_biased! {
            result = pin!(condition.fuse()) => {
                timer_cond.cancel();
                Ok(result)},
            _ = THREAD_TIME.with(|t| {t.clone().wait_until(expiry, timer_cond.clone()).fuse()}) => Err(Fail::new(libc::ETIMEDOUT, "a conditional wait timed out"))
        }
    } else {
        Ok(condition.await)
    }
}

/// Yield for one quanta.
pub async fn poll_yield() {
    let poll: PollFuture = PollFuture::default();
    poll.await;
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for SharedDemiRuntime {}

impl Default for SharedDemiRuntime {
    fn default() -> Self {
        let now: Instant = Instant::now();
        THREAD_TIME.with(|s| {
            s.clone().set_time(now);
        });

        Self(SharedObject::<DemiRuntime>::new(DemiRuntime {
            qtable: IoQueueTable::default(),
            ephemeral_ports: EphemeralPorts::default(),
            network_table: NetworkQueueTable::default(),
            ts_iters: 0,
            completed_tasks: HashMap::<QToken, (QDesc, OperationResult)>::new(),
        }))
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
