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

use crate::{
    collections::id_map::IdMap,
    runtime::{
        scheduler::{
            group::TaskGroup,
            Task,
            TaskId,
        },
        SharedObject,
    },
};
use ::slab::Slab;
use ::std::ops::{
    Deref,
    DerefMut,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Internal offset into the slab that holds the task state.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct InternalId(usize);

/// Task Scheduler
pub struct Scheduler {
    // Mapping between external task ids and internal ids (which currently represent the offset into the slab where the
    // task lives).
    ids: IdMap<TaskId, InternalId>,
    // A group of tasks used for resource management. Currently all of our tasks are in a single group but we will
    // eventually break them up by Demikernel queue for fairness and performance isolation.
    groups: Slab<TaskGroup>,
}

#[derive(Clone)]
pub struct SharedScheduler(SharedObject<Scheduler>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl Scheduler {
    /// Creates a new task group. Returns an identifier for the group.
    pub fn create_group(&mut self) -> TaskId {
        let internal_id: InternalId = self.groups.insert(TaskGroup::default()).into();
        let external_id: TaskId = self.ids.insert_with_new_id(internal_id);
        external_id
    }

    /// Get a reference to the task group using the id.
    fn get_group(&self, task_id: &TaskId) -> Option<&TaskGroup> {
        // Get the internal id of the parent task or group.
        let group_id: InternalId = self.ids.get(task_id)?;
        // Use that to find the task group for this task.
        self.groups.get(group_id.into())
    }

    /// Get a mutable reference to the task group using the id.
    fn get_mut_group(&mut self, task_id: &TaskId) -> Option<&mut TaskGroup> {
        // Get the internal id of the parent task or group.
        let group_id: InternalId = self.ids.get(task_id)?;
        // Use that to find the task group for this task.
        self.groups.get_mut(group_id.into())
    }

    /// Removes a task group. The group id should be the one originally allocated for this group since the group should
    /// not have any running tasks. Returns true if the task group was successfully removed.
    pub fn remove_group(&mut self, group_id: TaskId) -> bool {
        if let Some(internal_id) = self.ids.remove(&group_id) {
            self.groups.remove(internal_id.into());
            true
        } else {
            false
        }
    }

    /// Insert a task into a task group. The parent id can either be the id of the group or another task in the same
    /// group.
    pub fn insert_task<T: Task>(&mut self, group_id: TaskId, task: T) -> Option<TaskId> {
        // Get the internal id of the parent task or group.
        let internal_group_id: InternalId = self.ids.get(&group_id)?;
        // Allocate a new task id for the task.
        let new_task_id: TaskId = self.ids.insert_with_new_id(internal_group_id);
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = self.groups.get_mut(internal_group_id.into())?;
        // Insert the task into the task group.
        if group.insert(new_task_id, Box::new(task)) {
            Some(new_task_id)
        } else {
            None
        }
    }

    pub fn remove_task(&mut self, task_id: TaskId) -> Option<Box<dyn Task>> {
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = self.get_mut_group(&task_id)?;
        // Remove the task into the task group.
        let task: Box<dyn Task> = group.remove(task_id)?;
        // Remove the task mapping.
        self.ids.remove(&task_id)?;
        Some(task)
    }

    /// A high-level call for polling a single task. We only use this to poll a task synchronously immediately after
    /// scheduling it.
    pub fn poll_task(&mut self, task_id: TaskId) -> Option<Box<dyn Task>> {
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = self.get_mut_group(&task_id)?;
        group.poll_task(task_id)
    }

    /// Poll all tasks which are ready to run in a group at least once.
    pub fn poll_group(&mut self, group_id: &TaskId) -> Vec<Box<dyn Task>> {
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = match self.get_mut_group(&group_id) {
            Some(group) => group,
            None => return vec![],
        };
        group.poll_all()
    }

    /// Poll all tasks until one completes. Remove that task and return it or fail after polling [max_iteration] number
    /// of tasks.
    pub fn get_next_completed_task(
        &mut self,
        group_id: TaskId,
        max_iterations: usize,
    ) -> (usize, Option<Box<dyn Task>>) {
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = match self.get_mut_group(&group_id) {
            Some(group) => group,
            None => return (0, None),
        };
        group.get_next_completed_task(max_iterations)
    }

    #[allow(unused)]
    /// Returns whether this task id points to a valid task.
    pub fn is_valid_task(&self, task_id: &TaskId) -> bool {
        if let Some(group) = self.get_group(task_id) {
            group.is_valid_task(&task_id)
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn num_tasks(&self) -> usize {
        let mut num_tasks: usize = 0;
        for (_, group) in self.groups.iter() {
            num_tasks += group.num_tasks();
        }
        num_tasks
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            ids: IdMap::<TaskId, InternalId>::default(),
            groups: Slab::<TaskGroup>::default(),
        }
    }
}

impl Default for SharedScheduler {
    fn default() -> Self {
        Self(SharedObject::new(Scheduler::default()))
    }
}

impl Deref for SharedScheduler {
    type Target = Scheduler;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedScheduler {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl From<usize> for InternalId {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<InternalId> for usize {
    fn from(value: InternalId) -> Self {
        value.0
    }
}

impl From<u64> for InternalId {
    fn from(value: u64) -> Self {
        Self(value as usize)
    }
}

impl From<InternalId> for u64 {
    fn from(value: InternalId) -> Self {
        value.0 as u64
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::{
        expect_some,
        runtime::scheduler::{
            scheduler::{
                Scheduler,
                TaskId,
            },
            task::TaskWithResult,
        },
    };
    use ::anyhow::Result;
    use ::futures::FutureExt;
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

    /// This should never be used but ensures that the tests do not run forever.
    const MAX_ITERATIONS: usize = 100;

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
        let group_id: TaskId = scheduler.create_group();

        // Insert a task and make sure the task id is not a simple counter.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // Insert another task and make sure the task id is not sequentially after the previous one.
        let task2: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id2) = scheduler.insert_task(group_id, task2) else {
            anyhow::bail!("insert() failed")
        };

        crate::ensure_neq!(task_id2, task_id);

        Ok(())
    }

    #[test]
    fn poll_once_with_one_small_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        match scheduler.get_next_completed_task(group_id, 1) {
            (i, Some(task)) => {
                crate::ensure_eq!(task.get_id(), task_id);
                crate::ensure_eq!(i, 1);
            },
            (_, None) => anyhow::bail!("task should have completed"),
        }
        Ok(())
    }

    #[test]
    fn poll_next_with_one_small_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        match scheduler.get_next_completed_task(group_id, MAX_ITERATIONS) {
            (i, Some(task)) => {
                crate::ensure_eq!(task.get_id(), task_id);
                crate::ensure_eq!(i, 1);
            },
            (_, None) => anyhow::bail!("task should have completed"),
        }
        Ok(())
    }

    #[test]
    fn poll_twice_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete
        // with two poll operations.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(1).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, this future should make a transition.
        let (iterations, result) = scheduler.get_next_completed_task(group_id, 1);
        crate::ensure_eq!(result.is_some(), false);
        crate::ensure_eq!(iterations, 1);

        // This shall make the future ready.
        match scheduler.get_next_completed_task(group_id, 1) {
            (i, Some(task)) => {
                crate::ensure_eq!(task.get_id(), task_id);
                crate::ensure_eq!(i, 1);
            },
            (_, None) => anyhow::bail!("task should have completed"),
        }
        Ok(())
    }

    #[test]
    fn poll_next_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling until the task completes, our future should complete.
        match scheduler.get_next_completed_task(group_id, MAX_ITERATIONS) {
            (i, Some(task)) => {
                crate::ensure_eq!(task.get_id(), task_id);
                crate::ensure_eq!(i, 1);
            },
            (_, None) => anyhow::bail!("task should have completed"),
        }
        Ok(())
    }

    /// Tests if consecutive tasks are not assigned the same task id.
    #[test]
    fn insert_consecutive_creates_unique_task_ids() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        // Create and run a task.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        match scheduler.get_next_completed_task(group_id, 1) {
            (i, Some(task)) => {
                crate::ensure_eq!(task.get_id(), task_id);
                crate::ensure_eq!(i, 1);
            },
            (_, None) => anyhow::bail!("task should have completed"),
        }

        // Create another task.
        let task2: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id2) = scheduler.insert_task(group_id, task2) else {
            anyhow::bail!("insert() failed")
        };

        // Ensure that the second task has a unique id.
        crate::ensure_neq!(task_id2, task_id);

        Ok(())
    }

    #[test]
    fn remove_removes_task_id() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        // Arbitrarily large number.
        const NUM_TASKS: usize = 8192;
        let mut task_ids: Vec<TaskId> = Vec::<TaskId>::with_capacity(NUM_TASKS);

        crate::ensure_eq!(scheduler.num_tasks(), 0);

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(val).fuse()));
            let Some(task_id) = scheduler.insert_task(group_id, task) else {
                panic!("insert() failed");
            };
            task_ids.push(task_id);
        }

        // Remove tasks one by one and check if remove is only removing the task requested to be removed.
        let mut curr_num_tasks: usize = NUM_TASKS;
        for i in 0..NUM_TASKS {
            let task_id: TaskId = task_ids[i];
            // The id map does not dictate whether the id is valid, so we need to check the task slab as well.
            crate::ensure_eq!(true, scheduler.is_valid_task(&task_id));
            scheduler.remove_task(task_id);
            curr_num_tasks = curr_num_tasks - 1;
            crate::ensure_eq!(scheduler.num_tasks(), curr_num_tasks);
            // The id map does not dictate whether the id is valid, so we need to check the task slab as well.
            crate::ensure_eq!(false, scheduler.is_valid_task(&task_id));
        }

        crate::ensure_eq!(scheduler.num_tasks(), 0);

        Ok(())
    }

    #[bench]
    fn benchmark_insert(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        b.iter(|| {
            let task: DummyTask = DummyTask::new("testing", Box::pin(black_box(DummyCoroutine::default().fuse())));
            let task_id: TaskId = expect_some!(
                scheduler.insert_task(group_id, task),
                "couldn't insert future in scheduler"
            );
            black_box(task_id);
        });
    }

    #[bench]
    fn benchmark_poll(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        const NUM_TASKS: usize = 1024;
        let mut task_ids: Vec<TaskId> = Vec::<TaskId>::with_capacity(NUM_TASKS);
        let group_id: TaskId = scheduler.create_group();

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(val).fuse()));
            let Some(task_id) = scheduler.insert_task(group_id, task) else {
                panic!("insert() failed");
            };
            task_ids.push(task_id);
        }

        b.iter(|| {
            black_box(scheduler.poll_group(&group_id));
        });
    }

    #[bench]
    fn benchmark_next(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: TaskId = scheduler.create_group();

        const NUM_TASKS: usize = 1024;
        let mut task_ids: Vec<TaskId> = Vec::<TaskId>::with_capacity(NUM_TASKS);

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(val).fuse()));
            let Some(task_id) = scheduler.insert_task(group_id, task) else {
                panic!("insert() failed");
            };
            task_ids.push(task_id);
        }

        b.iter(|| {
            black_box(scheduler.get_next_completed_task(group_id, MAX_ITERATIONS));
        });
    }
}
