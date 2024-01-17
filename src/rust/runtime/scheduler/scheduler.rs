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
    runtime::scheduler::{
        group::TaskGroup,
        Task,
        TaskId,
    },
};
use ::slab::Slab;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Internal offset into the slab that holds the task state.
#[derive(Clone, Copy, Debug)]
pub struct InternalId(usize);

/// Task Scheduler
pub struct Scheduler {
    ids: IdMap<TaskId, InternalId>,
    groups: Slab<TaskGroup>,
    current_task: TaskId,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl Scheduler {
    /// Creates a new task group. Returns an identifier for the group.
    pub fn create_group(&mut self) -> TaskId {
        let internal_id: InternalId = self.groups.insert(TaskGroup::default()).into();
        self.ids.insert_with_new_id(internal_id)
    }

    pub fn switch_group(&mut self, group_id: TaskId) -> Option<TaskId> {
        if let Some(internal_id) = self.ids.get(&group_id) {
            if self.groups.contains(internal_id.into()) {
                let old_task: TaskId = self.current_task;
                self.current_task = group_id;
                return Some(old_task);
            }
        }
        None
    }

    fn get_group(&self, task_id: &TaskId) -> Option<&TaskGroup> {
        // Get the internal id of the parent task or group.
        let group_id: InternalId = self.ids.get(task_id)?;
        // Use that to find the task group for this task.
        self.groups.get(group_id.into())
    }

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
    pub fn insert_task<T: Task>(&mut self, task: T) -> Option<TaskId> {
        // Get the internal id of the parent task or group.
        let group_id: InternalId = self.ids.get(&self.current_task)?;
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = self.groups.get_mut(group_id.into())?;
        // Insert the task into the task group.
        let new_task_id: TaskId = group.insert(Box::new(task))?;
        // Add a mapping so we can use this new task id to find the task in the future.
        if let Some(existing) = self.ids.insert(new_task_id, group_id) {
            panic!("should not exist an id: {:?}", existing);
        }
        Some(new_task_id)
    }

    /// Insert a task into a task group. The parent id can either be the id of the group or another task in the same
    /// group.
    pub fn insert_task_with_group_id<T: Task>(&mut self, group_id: TaskId, task: T) -> Option<TaskId> {
        // Get the internal id of the parent task or group.
        let group_id: InternalId = self.ids.get(&group_id)?;
        // Use that to find the task group for this task.
        let group: &mut TaskGroup = self.groups.get_mut(group_id.into())?;
        // Insert the task into the task group.
        let new_task_id: TaskId = group.insert(Box::new(task))?;
        // Add a mapping so we can use this new task id to find the task in the future.
        self.ids.insert(new_task_id, group_id);
        Some(new_task_id)
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

    fn poll(&mut self, group_index: usize) -> usize {
        debug_assert!(group_index < self.groups.len());

        let mut polled_tasks: usize = 0;

        let ready_indices: Vec<usize> = self.groups[group_index].get_offsets_for_ready_tasks();
        for pin_slab_index in ready_indices {
            // Set the current running task for polling this task. This ensures that all tasks spawned by this task
            // will share the same task group.
            let old_task: TaskId = self.current_task;
            self.current_task = self.groups[group_index].get_id(pin_slab_index);
            self.groups[group_index].poll_notified_task(pin_slab_index);
            // Unset the current running task.
            self.current_task = old_task;
            polled_tasks += 1;
        }
        polled_tasks
    }

    /// Poll all tasks which are ready to run once. Tasks in our scheduler are notified when
    /// relevant data or events happen. The relevant event have callback function (the waker) which
    /// they can invoke to notify the scheduler that future should be polled again.
    pub fn poll_all(&mut self) -> usize {
        let mut polled_tasks: usize = 0;
        for i in 0..self.groups.len() {
            polled_tasks += self.poll(i);
        }
        polled_tasks
    }

    /// Poll all tasks in this group that are ready to run.
    pub fn poll_group(&mut self, group_id: TaskId) -> Option<usize> {
        Some(self.poll(self.ids.get(&group_id)?.into()))
    }

    pub fn has_completed(&self, task_id: TaskId) -> Option<bool> {
        // Use that to find the task group for this task.
        let group: &TaskGroup = self.get_group(&task_id)?;
        group.has_completed(task_id)
    }

    #[cfg(test)]
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
        let group: TaskGroup = TaskGroup::default();
        let mut ids: IdMap<TaskId, InternalId> = IdMap::<TaskId, InternalId>::default();
        let mut groups: Slab<TaskGroup> = Slab::<TaskGroup>::default();
        let internal_id: InternalId = groups.insert(group).into();
        // Use 0 as a special task id for the root.
        let current_task: TaskId = TaskId::from(0);
        ids.insert(current_task, internal_id);
        Self {
            ids,
            groups,
            current_task,
        }
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
    use crate::runtime::scheduler::{
        scheduler::{
            Scheduler,
            TaskId,
        },
        task::TaskWithResult,
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
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(task) else {
            anyhow::bail!("insert() failed")
        };

        // Insert another task and make sure the task id is not sequentially after the previous one.
        let task2: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id2) = scheduler.insert_task(task2) else {
            anyhow::bail!("insert() failed")
        };

        crate::ensure_neq!(task_id2, task_id);

        Ok(())
    }

    #[test]
    fn poll_once_with_one_small_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        scheduler.poll_all();

        crate::ensure_eq!(
            scheduler
                .has_completed(task_id)
                .expect("should find task completion status"),
            true
        );

        Ok(())
    }

    #[test]
    fn poll_twice_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete
        // with two poll operations.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(1).fuse()));
        let Some(task_id) = scheduler.insert_task(task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, this future should make a transition.
        scheduler.poll_all();

        crate::ensure_eq!(
            scheduler
                .has_completed(task_id)
                .expect("should find task completion status"),
            false
        );

        // This shall make the future ready.
        scheduler.poll_all();

        crate::ensure_eq!(
            scheduler
                .has_completed(task_id)
                .expect("should find task completion status"),
            true
        );

        Ok(())
    }

    /// Tests if consecutive tasks are not assigned the same task id.
    #[test]
    fn insert_consecutive_creates_unique_task_ids() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Create and run a task.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(task) else {
            anyhow::bail!("insert() failed")
        };
        scheduler.poll_all();

        // Ensure that the first task has completed.
        crate::ensure_eq!(
            scheduler
                .has_completed(task_id)
                .expect("should find task completion status"),
            true
        );

        // Create another task.
        let task2: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id2) = scheduler.insert_task(task2) else {
            anyhow::bail!("insert() failed")
        };

        // Ensure that the second task has a unique id.
        crate::ensure_neq!(task_id2, task_id);

        Ok(())
    }

    #[test]
    fn remove_removes_task_id() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Arbitrarily large number.
        const NUM_TASKS: usize = 8192;
        let mut task_ids: Vec<TaskId> = Vec::<TaskId>::with_capacity(NUM_TASKS);

        crate::ensure_eq!(scheduler.num_tasks(), 0);

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(val).fuse()));
            let Some(task_id) = scheduler.insert_task(task) else {
                panic!("insert() failed");
            };
            task_ids.push(task_id);
        }

        // This poll is required to give the opportunity for all the tasks to complete.
        scheduler.poll_all();

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

        b.iter(|| {
            let task: DummyTask = DummyTask::new(
                String::from("testing"),
                Box::pin(black_box(DummyCoroutine::default().fuse())),
            );
            let task_id: TaskId = scheduler
                .insert_task(task)
                .expect("couldn't insert future in scheduler");
            black_box(task_id);
        });
    }

    #[bench]
    fn benchmark_poll(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        const NUM_TASKS: usize = 1024;
        let mut task_ids: Vec<TaskId> = Vec::<TaskId>::with_capacity(NUM_TASKS);

        for val in 0..NUM_TASKS {
            let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(val).fuse()));
            let Some(task_id) = scheduler.insert_task(task) else {
                panic!("insert() failed");
            };
            task_ids.push(task_id);
        }

        b.iter(|| {
            black_box(scheduler.poll_all());
        });
    }
}
