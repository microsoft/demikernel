// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Implementation of our efficient, single-threaded task scheduler.
//!
//! Our scheduler uses a pinned memory slab to store tasks ([SchedulerFuture]s).
//! As background tasks are polled, they notify task in our scheduler via the
//! [crate::page::WakerPage]s.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::scheduler::{
    page::{
        WakerPageRef,
        WakerRef,
    },
    pin_slab::PinSlab,
    waker64::{
        WAKER_BIT_LENGTH,
        WAKER_BIT_LENGTH_SHIFT,
    },
    Task,
    TaskHandle,
};
use ::bit_iter::BitIter;
use ::rand::{
    rngs::SmallRng,
    RngCore,
    SeedableRng,
};
use ::std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    ptr::NonNull,
    task::{
        Context,
        Poll,
        Waker,
    },
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Seed for the random number generator used to generate tokens.
/// This value was chosen arbitrarily.
#[cfg(debug_assertions)]
const SCHEDULER_SEED: u64 = 42;
const MAX_NUM_TASKS: usize = 16000;
const MAX_RETRIES_TASK_ID_ALLOC: usize = 500;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Task Scheduler
pub struct Scheduler {
    /// Stores all the tasks that are held by the scheduler.
    tasks: PinSlab<Box<dyn Task>>,
    /// Maps between externally meaningful ids and the index of the task in the slab.
    task_ids: HashMap<u64, usize>,
    /// Holds the waker bits for controlling task scheduling.
    waker_page_refs: Vec<WakerPageRef>,
    /// Small random number generator for tokens.
    rng: SmallRng,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Scheduler
impl Scheduler {
    /// Given a handle to a task, remove it from the scheduler
    pub fn remove(&mut self, handle: &TaskHandle) -> Option<Box<dyn Task>> {
        let task_id: u64 = handle.get_task_id();
        // We should not have a scheduler handle that refers to an invalid id, so unwrap and expect are safe here.
        let pin_slab_index: usize = self
            .task_ids
            .remove(&task_id)
            .expect("Token should be in the token table");
        let (waker_page_ref, waker_page_offset): (&WakerPageRef, usize) = {
            let (waker_page_index, waker_page_offset) = self.get_waker_page_index_and_offset(pin_slab_index);
            (&self.waker_page_refs[waker_page_index], waker_page_offset)
        };
        waker_page_ref.clear(waker_page_offset);
        if let Some(task) = self.tasks.remove_unpin(pin_slab_index) {
            trace!(
                "remove(): name={:?}, id={:?}, pin_slab_index={:?}",
                task.get_name(),
                task_id,
                pin_slab_index
            );
            Some(task)
        } else {
            warn!(
                "Unable to unpin and remove: id={:?}, pin_slab_index={:?}",
                task_id, pin_slab_index
            );
            None
        }
    }

    /// Given a task id return a handle to the task.
    pub fn from_task_id(&self, task_id: u64) -> Option<TaskHandle> {
        let pin_slab_index: usize = match self.task_ids.get(&task_id) {
            Some(pin_slab_index) => *pin_slab_index,
            None => return None,
        };
        self.tasks.get(pin_slab_index)?;
        let waker_page_ref: &WakerPageRef = {
            let (waker_page_index, _) = self.get_waker_page_index_and_offset(pin_slab_index);
            &self.waker_page_refs[waker_page_index]
        };
        let waker_page_offset: usize = Scheduler::get_waker_page_offset(pin_slab_index);
        Some(TaskHandle::new(task_id, waker_page_ref.clone(), waker_page_offset))
    }

    /// Insert a new task into our scheduler returning a handle corresponding to it.
    pub fn insert<F: Task>(&mut self, future: F) -> Option<TaskHandle> {
        self.panic_if_too_many_tasks();

        let task_name: String = future.get_name();
        // The pin slab index can be reverse-computed in a page index and an offset within the page.
        let pin_slab_index: usize = self.tasks.insert(Box::new(future))?;
        let task_id: u64 = self.get_new_task_id(pin_slab_index);

        self.add_new_pages_up_to_pin_slab_index(pin_slab_index);

        // Initialize the appropriate page offset.
        let (waker_page_ref, waker_page_offset): (&WakerPageRef, usize) = {
            let (waker_page_index, waker_page_offset) = self.get_waker_page_index_and_offset(pin_slab_index);
            (&self.waker_page_refs[waker_page_index], waker_page_offset)
        };
        waker_page_ref.initialize(waker_page_offset);

        trace!(
            "insert(): name={:?}, id={:?}, pin_slab_index={:?}",
            task_name,
            task_id,
            pin_slab_index
        );
        Some(TaskHandle::new(task_id, waker_page_ref.clone(), waker_page_offset))
    }

    /// Generate a new id. If the id is currently in use, keep generating until we find an unused id.
    fn get_new_task_id(&mut self, pin_slab_index: usize) -> u64 {
        let new_task_id: u64 = 'get_id: {
            for _ in 0..MAX_RETRIES_TASK_ID_ALLOC {
                let new_task_id: u64 = self.rng.next_u64() as u16 as u64;
                if !self.task_ids.contains_key(&new_task_id) {
                    self.task_ids.insert(new_task_id, pin_slab_index);
                    break 'get_id new_task_id;
                }
            }
            panic!("Could not find a valid task id");
        };
        new_task_id
    }

    /// If the address space for task ids is close to half full, it will become increasingly difficult to avoid
    /// collisions, so we cap the number of tasks.
    fn panic_if_too_many_tasks(&self) {
        if self.task_ids.len() > MAX_NUM_TASKS {
            panic!("Too many concurrent tasks");
        }
    }

    /// Computes the page and page offset of a given task based on its total offset.
    fn get_waker_page_index_and_offset(&self, pin_slab_index: usize) -> (usize, usize) {
        let waker_page_index: usize = pin_slab_index >> WAKER_BIT_LENGTH_SHIFT;
        let waker_page_offset: usize = Scheduler::get_waker_page_offset(pin_slab_index);
        (waker_page_index, waker_page_offset)
    }

    /// Add new page(s) to hold this future's status if the current page is filled. This may result in addition of
    /// multiple pages because of the gap between the pin slab index and the current page index.
    fn add_new_pages_up_to_pin_slab_index(&mut self, pin_slab_index: usize) {
        while pin_slab_index >= (self.waker_page_refs.len() << WAKER_BIT_LENGTH_SHIFT) {
            self.waker_page_refs.push(WakerPageRef::default());
        }
    }

    /// Poll all futures which are ready to run again. Tasks in our scheduler are notified when
    /// relevant data or events happen. The relevant event have callback function (the waker) which
    /// they can invoke to notify the scheduler that future should be polled again.
    pub fn poll(&mut self) {
        let num_waker_pages = self.get_num_waker_pages();
        for waker_page_index in 0..num_waker_pages {
            let notified_offsets: u64 = self.get_offsets_for_ready_tasks(waker_page_index);
            self.poll_notified_tasks(waker_page_index, notified_offsets);
        }
    }

    fn get_num_waker_pages(&self) -> usize {
        self.waker_page_refs.len()
    }

    fn get_offsets_for_ready_tasks(&mut self, waker_page_index: usize) -> u64 {
        let waker_page_ref: &mut WakerPageRef = &mut self.waker_page_refs[waker_page_index];
        waker_page_ref.take_notified()
    }

    fn poll_notified_tasks(&mut self, waker_page_index: usize, notified_offsets: u64) {
        for waker_page_offset in BitIter::from(notified_offsets) {
            // Get the pinned ref.
            let pinned_ptr = {
                let pin_slab_index: usize = Scheduler::get_pin_slab_index(waker_page_index, waker_page_offset);
                let pinned_ref: Pin<&mut Box<dyn Task>> = self.tasks.get_pin_mut(pin_slab_index).unwrap();
                let pinned_ptr = unsafe { Pin::into_inner_unchecked(pinned_ref) as *mut _ };
                pinned_ptr
            };
            let pinned_ref = unsafe { Pin::new_unchecked(&mut *pinned_ptr) };

            // Get the waker context.
            let waker: Waker = unsafe {
                let raw_waker: NonNull<u8> =
                    self.waker_page_refs[waker_page_index].into_raw_waker_ref(waker_page_offset);
                Waker::from_raw(WakerRef::new(raw_waker).into())
            };
            let mut waker_context: Context = Context::from_waker(&waker);

            // Poll future.
            let poll_result: Poll<()> = Future::poll(pinned_ref, &mut waker_context);
            if let Poll::Ready(()) = poll_result {
                self.waker_page_refs[waker_page_index].mark_completed(waker_page_offset)
            }
        }
    }

    fn get_waker_page_offset(pin_slab_index: usize) -> usize {
        pin_slab_index & (WAKER_BIT_LENGTH - 1)
    }

    fn get_pin_slab_index(waker_page_index: usize, waker_page_offset: usize) -> usize {
        (waker_page_index << WAKER_BIT_LENGTH_SHIFT) + waker_page_offset
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Default Trait Implementation for Scheduler
impl Default for Scheduler {
    /// Creates a scheduler with default values.
    fn default() -> Self {
        Self {
            tasks: PinSlab::new(),
            task_ids: HashMap::<u64, usize>::new(),
            waker_page_refs: vec![],
            #[cfg(debug_assertions)]
            rng: SmallRng::seed_from_u64(SCHEDULER_SEED),
            #[cfg(not(debug_assertions))]
            rng: SmallRng::from_entropy(),
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        scheduler::{
            Scheduler,
            TaskHandle,
        },
        task::TaskWithResult,
    };
    use ::anyhow::Result;
    use ::std::{
        future::Future,
        pin::Pin,
        task::{
            Context,
            Poll,
            Waker,
        },
    };
    use ::test::{
        black_box,
        Bencher,
    };

    #[derive(Default)]
    struct DummyCoroutine {
        pub val: usize,
    }

    impl DummyCoroutine {
        pub fn new(val: usize) -> Self {
            let f: Self = Self { val };
            f
        }
    }
    impl Future for DummyCoroutine {
        type Output = ();

        fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
            match self.as_ref().val & 1 {
                0 => Poll::Ready(()),
                _ => {
                    self.get_mut().val += 1;
                    let waker: &Waker = ctx.waker();
                    waker.wake_by_ref();
                    Poll::Pending
                },
            }
        }
    }

    type DummyTask = TaskWithResult<()>;

    /// Tests if when inserting multiple tasks into the scheduler at once each, of them gets a unique identifier.
    #[test]
    fn insert_creates_unique_tasks_ids() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a task and make sure the task id is not a simple counter.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0)));
        let handle: TaskHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };
        let task_id: u64 = handle.get_task_id();

        // Insert another task and make sure the task id is not sequentially after the previous one.
        let task2: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0)));
        let handle2: TaskHandle = match scheduler.insert(task2) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };
        let task_id2: u64 = handle2.get_task_id();
        crate::ensure_neq!(task_id2, task_id);

        Ok(())
    }

    #[test]
    fn poll_once_with_one_small_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0)));
        let handle: TaskHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        scheduler.poll();

        crate::ensure_eq!(handle.has_completed(), true);

        Ok(())
    }

    #[test]
    fn poll_twice_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete
        // with two poll operations.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(1)));
        let handle: TaskHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, this future should make a transition.
        scheduler.poll();

        crate::ensure_eq!(handle.has_completed(), false);

        // This shall make the future ready.
        scheduler.poll();

        crate::ensure_eq!(handle.has_completed(), true);

        Ok(())
    }

    /// Tests if consecutive tasks are not assigned the same task id.
    #[test]
    fn insert_consecutive_creates_unique_task_ids() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Create and run a task.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0)));
        let handle: TaskHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };
        let task_id: u64 = handle.clone().get_task_id();
        scheduler.poll();

        // Ensure that the first task has completed.
        crate::ensure_eq!(handle.has_completed(), true);

        // Create another task.
        let task2: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0)));
        let handle2: TaskHandle = match scheduler.insert(task2) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };
        let task_id2: u64 = handle2.get_task_id();

        // Ensure that the second task has a unique id.
        crate::ensure_neq!(task_id2, task_id);

        Ok(())
    }

    #[test]
    fn remove_removes_task_id() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        // Arbitrarily large number.
        const NUM_TASKS: usize = 8192;
        let mut handles: Vec<TaskHandle> = Vec::<TaskHandle>::with_capacity(NUM_TASKS);

        crate::ensure_eq!(true, scheduler.task_ids.is_empty());

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(val)));
            let handle: TaskHandle = match scheduler.insert(task) {
                Some(handle) => handle,
                None => panic!("insert() failed"),
            };
            handles.push(handle);
        }

        // This poll is required to give the opportunity for all the tasks to complete.
        scheduler.poll();

        // Remove tasks one by one and check if remove is only removing the task requested to be removed.
        let mut curr_num_tasks: usize = NUM_TASKS;
        for i in 0..NUM_TASKS {
            let task_id: u64 = handles[i].get_task_id();
            crate::ensure_eq!(true, scheduler.task_ids.contains_key(&task_id));
            scheduler.remove(&handles[i]);
            curr_num_tasks = curr_num_tasks - 1;
            crate::ensure_eq!(scheduler.task_ids.len(), curr_num_tasks);
            crate::ensure_eq!(false, scheduler.task_ids.contains_key(&task_id));
        }

        crate::ensure_eq!(scheduler.task_ids.is_empty(), true);

        Ok(())
    }

    #[test]
    fn from_task_id_returns_none_for_non_existing_task_id() -> Result<()> {
        let scheduler: Scheduler = Scheduler::default();
        match scheduler.from_task_id(0) {
            Some(_) => anyhow::bail!("from_task_id() must return None"),
            None => {},
        }
        Ok(())
    }

    #[test]
    fn from_task_id_returns_correct_task_handle() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(42)));
        let handle: TaskHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => anyhow::bail!("insert() failed"),
        };
        let task_id: u64 = handle.get_task_id();
        match scheduler.from_task_id(task_id) {
            Some(retreived_task_handle) => crate::ensure_eq!(task_id, retreived_task_handle.get_task_id()),
            None => anyhow::bail!("from_task_id() must not return None"),
        }
        Ok(())
    }

    #[bench]
    fn benchmark_insert(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();

        b.iter(|| {
            let task: DummyTask =
                DummyTask::new(String::from("testing"), Box::pin(black_box(DummyCoroutine::default())));
            let handle: TaskHandle = scheduler.insert(task).expect("couldn't insert future in scheduler");
            black_box(handle);
        });
    }

    #[bench]
    fn benchmark_poll(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        const NUM_TASKS: usize = 1024;
        let mut handles: Vec<TaskHandle> = Vec::<TaskHandle>::with_capacity(NUM_TASKS);

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(val)));
            let handle: TaskHandle = match scheduler.insert(task) {
                Some(handle) => handle,
                None => panic!("insert() failed"),
            };
            handles.push(handle);
        }

        b.iter(|| {
            black_box(scheduler.poll());
        });
    }
}
