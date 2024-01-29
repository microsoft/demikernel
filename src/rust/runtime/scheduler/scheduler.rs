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
use ::std::{
    ops::{
        Deref,
        DerefMut,
    },
    task::Waker,
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
    // Track the currently running task id. This is entirely for external use. If there are no coroutines running (i.
    // e.g, we did not enter the scheduler through a wait), this MUST be set to none because we cannot yield or wake ///
    // unless inside a task/async coroutine.
    current_running_task: Box<Option<TaskId>>,

    // These global variables are for our scheduling policy. For now, we simply use round robin.
    // The index of the current or last task that we ran.
    current_task_id: InternalId,
    // The group index of the current or last task that we ran.
    current_group_id: InternalId,
    // The current set of ready tasks in the group.
    current_ready_tasks: Vec<InternalId>,
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
        self.ids.insert_with_new_id(internal_id)
    }

    /// Switch to a different task group. Returns true if the group has been switched.
    pub fn switch_group(&mut self, group_id: TaskId) -> bool {
        if let Some(internal_id) = self.ids.get(&group_id) {
            if self.groups.contains(internal_id.into()) {
                self.current_group_id = internal_id;
                return true;
            }
        }
        false
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
    pub fn insert_task<T: Task>(&mut self, task: T) -> Option<TaskId> {
        // Use the currently running task id to find the task group for this task.
        let group: &mut TaskGroup = self.groups.get_mut(self.current_group_id.into())?;
        // Insert the task into the task group.
        let new_task_id: TaskId = group.insert(Box::new(task))?;
        // Add a mapping so we can use this new task id to find the task in the future.
        if let Some(existing) = self.ids.insert(new_task_id, self.current_group_id) {
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

    fn poll_notified_task_and_remove_if_ready(&mut self) -> Option<Box<dyn Task>> {
        let group: &mut TaskGroup = self
            .groups
            .get_mut(self.current_group_id.into())
            .expect("task group should exist: ");
        assert!(self.current_running_task.is_none());
        *self.current_running_task = Some(group.unchecked_internal_to_external_id(self.current_task_id));
        assert!(self.current_running_task.is_some());
        let result: Option<Box<dyn Task>> = group.poll_notified_task_and_remove_if_ready(self.current_task_id);
        assert!(self.current_running_task.is_some());
        *self.current_running_task = None;
        assert!(self.current_running_task.is_none());
        result
    }

    /// Poll all tasks which are ready to run for [max_iterations]. This does the same thing as get_next_completed task
    /// but does not stop until it has reached [max_iterations] and collects all of the
    pub fn poll_all(&mut self) -> Vec<Box<dyn Task>> {
        let mut completed_tasks: Vec<Box<dyn Task>> = vec![];
        let start_group = self.current_group_id;
        loop {
            self.current_task_id = {
                match self.current_ready_tasks.pop() {
                    Some(index) => index,
                    None => {
                        self.next_runnable_group();
                        if self.current_group_id == start_group {
                            break;
                        }
                        match self.current_ready_tasks.pop() {
                            Some(index) => index,
                            None => return completed_tasks,
                        }
                    },
                }
            };

            // Now that we have a runnable task, actually poll it.
            if let Some(task) = self.poll_notified_task_and_remove_if_ready() {
                completed_tasks.push(task);
            }
        }
        completed_tasks
    }

    /// Poll all tasks until one completes. Remove that task and return it or fail after polling [max_iteration] number
    /// of tasks.
    pub fn get_next_completed_task(&mut self, max_iterations: usize) -> Option<Box<dyn Task>> {
        for _ in 0..max_iterations {
            self.current_task_id = {
                match self.current_ready_tasks.pop() {
                    Some(index) => index,
                    None => {
                        self.next_runnable_group();
                        match self.current_ready_tasks.pop() {
                            Some(index) => index,
                            None => return None,
                        }
                    },
                }
            };

            // Now that we have a runnable task, actually poll it.
            if let Some(task) = self.poll_notified_task_and_remove_if_ready() {
                return Some(task);
            }
        }
        None
    }

    /// Poll over all of the groups looking for a group with runnable tasks. Sets the current_group_id to the next
    /// runnable task group and current_ready_tasks to a list of tasks that are runnable in that group.
    fn next_runnable_group(&mut self) {
        let starting_group_index: InternalId = self.current_group_id;
        self.current_group_id = self.get_next_group_index();

        loop {
            self.current_ready_tasks = self.groups[self.current_group_id.into()].get_offsets_for_ready_tasks();
            if !self.current_ready_tasks.is_empty() {
                return;
            }
            // If we reach this point, then we have looped all the way around without finding any runnable tasks.
            if self.current_group_id == starting_group_index {
                return;
            }
        }
    }

    /// Choose the index of the next group to run.
    fn get_next_group_index(&self) -> InternalId {
        // For now, we just choose the next group in the list.
        InternalId::from((usize::from(self.current_group_id) + 1) % self.groups.len())
    }

    /// Returns whether this task id points to a valid task.
    pub fn is_valid_task(&self, task_id: &TaskId) -> bool {
        if let Some(group) = self.get_group(task_id) {
            group.is_valid_task(&task_id)
        } else {
            false
        }
    }

    /// Returns the current running task id if we are in the scheduler, otherwise None.
    pub fn get_task_id(&self) -> Option<TaskId> {
        *self.current_running_task.clone()
    }

    #[allow(unused)]
    pub fn get_waker(&self, task_id: TaskId) -> Option<Waker> {
        let group: &TaskGroup = self.get_group(&task_id)?;
        let internal_id: InternalId = group.unchecked_external_to_internal_id(&task_id);
        group.get_waker(internal_id)
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
            current_running_task: Box::new(None),
            current_group_id: internal_id,
            current_task_id: InternalId(0),
            current_ready_tasks: vec![],
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
        if let Some(task) = scheduler.get_next_completed_task(1) {
            crate::ensure_eq!(task.get_id(), task_id);
        } else {
            anyhow::bail!("task should have completed");
        }
        Ok(())
    }

    #[test]
    fn poll_next_with_one_small_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        if let Some(task) = scheduler.get_next_completed_task(MAX_ITERATIONS) {
            crate::ensure_eq!(task_id, task.get_id());
            Ok(())
        } else {
            anyhow::bail!("task should have completed")
        }
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
        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        let result = scheduler.get_next_completed_task(1);
        crate::ensure_eq!(result.is_some(), false);

        // This shall make the future ready.
        if let Some(task) = scheduler.get_next_completed_task(1) {
            crate::ensure_eq!(task.get_id(), task_id);
        } else {
            anyhow::bail!("task should have completed");
        }
        Ok(())
    }

    #[test]
    fn poll_next_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new(String::from("testing"), Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling until the task completes, our future should complete.
        if let Some(task) = scheduler.get_next_completed_task(MAX_ITERATIONS) {
            crate::ensure_eq!(task_id, task.get_id());
            Ok(())
        } else {
            anyhow::bail!("task should have completed")
        }
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

        if let Some(task) = scheduler.get_next_completed_task(1) {
            crate::ensure_eq!(task.get_id(), task_id);
        } else {
            anyhow::bail!("task should have completed");
        }

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

    #[bench]
    fn benchmark_next(b: &mut Bencher) {
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
            black_box(scheduler.get_next_completed_task(MAX_ITERATIONS));
        });
    }
}
