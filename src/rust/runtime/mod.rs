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
pub mod tracing;
pub mod types;
pub use condition_variable::SharedConditionVariable;
mod poll;
pub mod timer;
pub use queue::{BackgroundTask, OperationResult, OperationTask, QDesc, QToken, QType};
pub use scheduler::{SchedulerId, Task};

#[cfg(feature = "libdpdk")]
pub use demikernel_dpdk_bindings as libdpdk;

#[cfg(feature = "libxdp")]
pub use demikernel_xdp_bindings as libxdp;

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(feature = "profiler")]
use crate::coroutine_timer;

use crate::{
    collections::id_map::Id64Map,
    expect_some,
    runtime::{
        fail::Fail,
        network::{socket::SocketId, SocketIdToQDescMap},
        poll::PollFuture,
        queue::{IoQueue, IoQueueTable},
        scheduler::{SharedScheduler, TaskWithResult},
    },
};
use ::futures::{future::FusedFuture, select_biased, Future, FutureExt};

use ::std::{
    any::Any,
    collections::HashMap,
    ops::{Deref, DerefMut},
    pin::pin,
    rc::Rc,
    time::{Duration, Instant},
};
use ::std::{collections::VecDeque, pin::Pin};

//======================================================================================================================
// Constants
//======================================================================================================================

// TODO: Make this more accurate using rdtsc.
// FIXME: https://github.com/microsoft/demikernel/issues/1226
const TIMER_RESOLUTION: usize = 64;
const DEFAULT_TASK_STORAGE_CAPACITY: usize = 1024;
const DEFAULT_RESULT_STORAGE_CAPACITY: usize = 1024;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct DemiRuntime {
    qtable: IoQueueTable,
    // Holds the mapping between qtoken and task id. Initialized to invalid id until we insert the task into the
    // scheduler and get the real id.
    qtoken_to_scheduler_id: Id64Map<QToken, SchedulerId>,
    scheduler: SharedScheduler,
    foreground_group_id: SchedulerId,
    background_group_id: SchedulerId,
    socket_id_to_qdesc_map: SocketIdToQDescMap,
    /// Number of iterations that we have polled since advancing the clock.
    ts_iters: usize,
    /// Tasks that have been completed and removed from the scheduler
    completed_tasks: VecDeque<OperationTask>,
    completed_results: HashMap<QToken, (QDesc, OperationResult)>,
}

#[derive(Clone)]
pub struct SharedDemiRuntime(SharedObject<DemiRuntime>);

#[derive(Default)]
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
        timer::global_set_time(now);
        let mut scheduler: SharedScheduler = SharedScheduler::default();
        let foreground_group_id: SchedulerId = scheduler.create_group();
        let background_group_id: SchedulerId = scheduler.create_group();
        Self(SharedObject::<DemiRuntime>::new(DemiRuntime {
            qtable: IoQueueTable::default(),
            qtoken_to_scheduler_id: Id64Map::default(),
            scheduler,
            foreground_group_id,
            background_group_id,
            socket_id_to_qdesc_map: SocketIdToQDescMap::default(),
            ts_iters: 0,
            completed_tasks: VecDeque::with_capacity(DEFAULT_TASK_STORAGE_CAPACITY),
            completed_results: HashMap::with_capacity(DEFAULT_RESULT_STORAGE_CAPACITY),
        }))
    }

    /// Inserts the background `coroutine` named `task_name` into the scheduler. There should only be one of these
    /// because we should never be polling with more than one coroutine.
    pub fn insert_io_polling_coroutine<F: FusedFuture<Output = ()> + 'static>(
        &mut self,
        task_name: &'static str,
        coroutine: Pin<Box<F>>,
    ) -> Result<QToken, Fail> {
        self.insert_coroutine(task_name, self.background_group_id, coroutine)
    }

    /// Inserts the background `coroutine` named `task_name` into the scheduler. There should only be one of these
    /// because we should never be polling with more than one coroutine.
    pub fn insert_nonpolling_coroutine<F: FusedFuture + 'static>(
        &mut self,
        task_name: &'static str,
        coroutine: Pin<Box<F>>,
    ) -> Result<QToken, Fail>
    where
        F::Output: Unpin + Clone + Any,
    {
        self.insert_coroutine(task_name, self.foreground_group_id, coroutine)
    }

    /// Inserts a coroutine of type T and task
    fn insert_coroutine<F: FusedFuture + 'static>(
        &mut self,
        task_name: &'static str,
        group_id: SchedulerId,
        coroutine: Pin<Box<F>>,
    ) -> Result<QToken, Fail>
    where
        F::Output: Unpin + Clone + Any,
    {
        trace!("Inserting coroutine: {:?}", task_name);
        #[cfg(feature = "profiler")]
        let coroutine = coroutine_timer!(task_name, coroutine);
        let task: TaskWithResult<F::Output> = TaskWithResult::<F::Output>::new(task_name, coroutine);
        match self.scheduler.insert_task(group_id, task) {
            Some(task_id) => {
                let qt: QToken = self.qtoken_to_scheduler_id.insert_with_new_id(task_id).unwrap();
                self.scheduler
                    .get_mut_task(group_id, task_id)
                    .unwrap()
                    .set_id(qt.into());
                // Stash the newly allocated qtoken in the task.
                Ok(qt)
            },
            None => {
                let cause: String = format!("cannot schedule coroutine (task_name={:?})", &task_name);
                error!("insert_nonpolling_coroutine(): {}", cause);
                Err(Fail::new(libc::EAGAIN, &cause))
            },
        }
    }

    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Duration) -> Result<(QDesc, OperationResult), Fail> {
        trace!(
            "wait(): qt={:?}, timeout={:?} len={:?}",
            qt,
            timeout,
            self.completed_tasks.len()
        );
        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];

        // Call wait_any() to do the real work.
        let (offset, returned_qt, qd, result) = self.wait_any(&qt_array, timeout)?;
        debug_assert_eq!(offset, 0);
        debug_assert_eq!(qt, returned_qt);
        Ok((qd, result))
    }

    /// Waits until one of the tasks in qts has completed and returns the result.
    pub fn wait_any(
        &mut self,
        qts: &[QToken],
        timeout: Duration,
    ) -> Result<(usize, QToken, QDesc, OperationResult), Fail> {
        // If any are already complete, grab from the completion table, otherwise, make sure it is valid.
        for (i, qt) in qts.iter().enumerate() {
            if let Some((qd, result)) = self.completed_results.remove(qt) {
                return Ok((i, *qt, qd, result));
            }
            if self.qtoken_to_scheduler_id.get(qt).is_none() {
                return Err(Fail::new(libc::EINVAL, "invalid qtoken"));
            }
        }

        let mut result: Option<(usize, QToken, QDesc, OperationResult)> = None;
        let mut unused_results: Vec<(QToken, QDesc, OperationResult)> = vec![];
        // Wait until one of the qts is ready.
        let return_value: Result<(), Fail> = self.wait_next_n(
            |completed_qt, completed_qd, completed_result| match qts
                .iter()
                .enumerate()
                .find(|(_, qt)| completed_qt == **qt)
            {
                Some((i, _)) => {
                    result = Some((i, completed_qt, completed_qd, completed_result));
                    false
                },
                None => {
                    unused_results.push((completed_qt, completed_qd, completed_result));
                    true
                },
            },
            timeout,
        );
        unused_results
            .drain(..)
            .for_each(|(completed_qt, completed_qd, completed_result)| {
                self.completed_results
                    .insert(completed_qt, (completed_qd, completed_result));
            });
        match return_value {
            Ok(()) => Ok(result.unwrap()),
            Err(e) => Err(e),
        }
    }

    /// Waits until the next task is complete, passing the result to `acceptor`. The acceptor consumes the result,
    /// which is not further stored. The qtoken mapping is also removed. The acceptor may return true to
    /// continue waiting or false to exit the wait. The method will return when either the acceptor returns false
    /// (returning Ok) or the timeout has expired (returning a Fail indicating timeout).
    pub fn wait_next_n<Acceptor: FnMut(QToken, QDesc, OperationResult) -> bool>(
        &mut self,
        mut acceptor: Acceptor,
        timeout: Duration,
    ) -> Result<(), Fail> {
        let deadline_time: Instant = self.get_now() + timeout;
        loop {
            if self.completed_tasks.is_empty() {
                self.run_scheduler();
            }
            while let Some(mut task) = self.completed_tasks.pop_front() {
                let qt: QToken = expect_some!(task.get_id(), "should have been set on insert").into();
                let (qd, result): (QDesc, OperationResult) = expect_some!(task.get_result(), "coroutine not finished");
                self.qtoken_to_scheduler_id.remove(&qt);

                if !acceptor(qt, qd, result) {
                    return Ok(());
                }
            }
            if self.get_now() >= deadline_time {
                break;
            } else {
                self.advance_clock_to_now();
            }
        }

        Err(Fail::new(libc::ETIMEDOUT, "wait timed out"))
    }

    /// Either pull the first task from the completed task queue or run the scheduler for one iteration to populate the
    /// completed task queue and then try again. Returns None if there are still no completed tasks after polling the
    /// scheduler.
    pub fn run_scheduler(&mut self) {
        // 1. Run each of the background tasks once.
        self.poll_background_tasks();
        // 2. Run all foreground tasks until none are ready.
        let completed_tasks: Vec<OperationTask> = self.poll_foreground_tasks();

        // 3. Update the list of previously completed tasks.
        self.completed_tasks = VecDeque::from(completed_tasks);
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

    /// Moves time forward deterministically.
    pub fn advance_clock(&mut self, now: Instant) {
        timer::global_advance_clock(now)
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
        timer::global_get_time()
    }

    /// Checks if an identifier is in use and returns the queue descriptor if it is.
    pub fn get_qd_from_socket_id(&self, id: &SocketId) -> Option<QDesc> {
        match self.socket_id_to_qdesc_map.get_qd(id) {
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
        self.socket_id_to_qdesc_map.insert(id, qd)
    }

    /// Removes a mapping and returns the mapped queue descriptor.
    pub fn remove_socket_id_to_qd(&mut self, id: &SocketId) -> Option<QDesc> {
        match self.socket_id_to_qdesc_map.remove(id) {
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

    pub fn poll_background_tasks(&mut self) {
        let background_group_id: SchedulerId = self.background_group_id;
        // Ignore any results from tasks that completed because background tasks do not return anything.
        self.scheduler
            .poll_group_once(background_group_id, Some(TIMER_RESOLUTION));
    }

    pub fn poll_foreground_tasks(&mut self) -> Vec<OperationTask> {
        let foreground_group_id: SchedulerId = self.foreground_group_id;

        let completed_tasks = self
            .scheduler
            .poll_group_until_unrunnable(foreground_group_id, Some(TIMER_RESOLUTION));

        completed_tasks
            .into_iter()
            .filter_map(|boxed_task| -> Option<OperationTask> {
                let qt: QToken = expect_some!(boxed_task.get_id(), "should have been set on insert").into();
                trace!(
                    "Completed while polling coroutine (qt={:?}): {:?}",
                    qt,
                    boxed_task.get_name()
                );

                // OperationTasks return a value to the application, so we must stash these for later. Otherwise, we
                // just discard the return value of the completed coroutine.
                if let Ok(operation_task) = OperationTask::try_from(boxed_task.as_any()) {
                    Some(operation_task)
                } else {
                    None
                }
            })
            .collect()
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

pub async fn yield_with_timeout(timeout: Duration) {
    timer::wait(timeout).await
}

/// Yield until either the condition completes or we time out. If the timeout is 0, then run
pub async fn conditional_yield_with_timeout<F: Future>(condition: F, timeout: Duration) -> Result<F::Output, Fail> {
    select_biased! {
        result = pin!(condition.fuse()) => Ok(result),
        _ = timer::wait(timeout).fuse() => Err(Fail::new(libc::ETIMEDOUT, "a conditional wait timed out"))
    }
}

/// Yield until either the condition completes or the [expiry] time passes. If the expiry time is  then wait until
/// the condition completes.
pub async fn conditional_yield_until<F: Future>(condition: F, expiry: Option<Instant>) -> Result<F::Output, Fail> {
    if let Some(expiry) = expiry {
        select_biased! {
            result = pin!(condition.fuse()) => Ok(result),
            _ = timer::wait_until(expiry).fuse() => Err(Fail::new(libc::ETIMEDOUT, "a conditional wait timed out"))
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

impl Default for SharedDemiRuntime {
    fn default() -> Self {
        timer::global_set_time(Instant::now());
        let mut scheduler: SharedScheduler = SharedScheduler::default();
        let foreground_group_id: SchedulerId = scheduler.create_group();
        let background_group_id: SchedulerId = scheduler.create_group();
        Self(SharedObject::<DemiRuntime>::new(DemiRuntime {
            qtable: IoQueueTable::default(),
            qtoken_to_scheduler_id: Id64Map::default(),
            scheduler,
            foreground_group_id,
            background_group_id,
            socket_id_to_qdesc_map: SocketIdToQDescMap::default(),
            ts_iters: 0,
            completed_results: HashMap::with_capacity(DEFAULT_RESULT_STORAGE_CAPACITY),
            completed_tasks: VecDeque::with_capacity(DEFAULT_TASK_STORAGE_CAPACITY),
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

//======================================================================================================================
// Benchmarks
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::{
        ensure_eq, expect_ok,
        runtime::{poll_yield, OperationResult, QDesc, QToken, SharedDemiRuntime},
    };
    use ::anyhow::Result;
    use ::futures::FutureExt;
    use ::std::time::Duration;
    use ::test::Bencher;

    async fn dummy_coroutine(iterations: usize, qd: QDesc) -> (QDesc, OperationResult) {
        for _ in 0..iterations {
            poll_yield().await;
        }
        (qd, OperationResult::Close)
    }

    async fn dummy_background_coroutine() {
        loop {
            poll_yield().await
        }
    }

    #[test]
    fn poll_and_wait_for_first_task() -> Result<()> {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        let qt: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(0)).fuse()))?;
        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(1)).fuse()))?;
        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(2)).fuse()))?;

        let task = runtime.wait(qt, Duration::ZERO)?;
        ensure_eq!(task.0, QDesc::from(0));
        Ok(())
    }

    #[test]
    fn poll_and_wait_for_middle_task() -> Result<()> {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(0)).fuse()))?;
        let qt2: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(1)).fuse()))?;
        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(2)).fuse()))?;

        let task = runtime.wait(qt2, Duration::ZERO)?;
        ensure_eq!(task.0, QDesc::from(1));
        Ok(())
    }

    #[test]
    fn poll_and_wait_for_last_task() -> Result<()> {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(0)).fuse()))?;
        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(1)).fuse()))?;
        let qt3: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(2)).fuse()))?;

        let task = runtime.wait(qt3, Duration::ZERO)?;
        ensure_eq!(task.0, QDesc::from(2));
        Ok(())
    }

    #[test]
    fn poll_and_wait_any_for_first_tasks() -> Result<()> {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        let qt: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(0)).fuse()))?;
        let qt2: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(1)).fuse()))?;
        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(2)).fuse()))?;
        let qts: Vec<QToken> = vec![qt, qt2];
        let task = runtime.wait_any(&qts, Duration::ZERO)?;
        ensure_eq!(task.2, QDesc::from(0));
        Ok(())
    }

    #[test]
    fn poll_and_wait_any_for_last_tasks() -> Result<()> {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        let _: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(0)).fuse()))?;
        let qt2: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(1)).fuse()))?;
        let qt3: QToken = runtime
            .insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(1, QDesc::from(2)).fuse()))?;
        let qts: Vec<QToken> = vec![qt2, qt3];
        let task = runtime.wait_any(&qts, Duration::ZERO)?;
        ensure_eq!(task.2, QDesc::from(1));
        Ok(())
    }

    #[bench]
    fn insert_io_coroutine_bench(b: &mut Bencher) {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        b.iter(|| {
            runtime.insert_nonpolling_coroutine("dummy coroutine", Box::pin(dummy_coroutine(10, QDesc::from(0)).fuse()))
        });
    }

    #[bench]
    fn insert_background_coroutine_bench(b: &mut Bencher) {
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();

        b.iter(|| {
            runtime.insert_nonpolling_coroutine(
                "dummy background coroutine",
                Box::pin(dummy_background_coroutine().fuse()),
            )
        });
    }

    #[bench]
    fn wait_any_nonpolling_coroutine_bench(b: &mut Bencher) {
        const NUM_TASKS: usize = 1024;
        let mut qts: [QToken; NUM_TASKS] = [QToken::from(0); NUM_TASKS];
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        // Insert a large number of coroutines.
        for i in 0..NUM_TASKS {
            qts[i] = expect_ok!(
                runtime.insert_nonpolling_coroutine(
                    "dummy coroutine",
                    Box::pin(dummy_coroutine(1000000000, QDesc::from(0)).fuse())
                ),
                "should be able to insert tasks"
            );
        }

        // Run all of the tasks for one quanta
        b.iter(|| runtime.wait_any(&qts, Duration::ZERO));
    }

    #[bench]
    fn wait_any_io_polling_coroutine_bench(b: &mut Bencher) {
        const NUM_TASKS: usize = 1024;
        let mut qts: [QToken; NUM_TASKS] = [QToken::from(0); NUM_TASKS];
        let mut runtime: SharedDemiRuntime = SharedDemiRuntime::default();
        // Insert a large number of coroutines.
        for i in 0..NUM_TASKS {
            qts[i] = expect_ok!(
                runtime.insert_io_polling_coroutine(
                    "dummy background coroutine",
                    Box::pin(dummy_background_coroutine().fuse()),
                ),
                "should be able to insert tasks"
            );
        }

        // Run all of the tasks for one quanta
        b.iter(|| runtime.wait_any(&qts, Duration::ZERO));
    }
}
