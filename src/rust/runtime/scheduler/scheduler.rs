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

use crate::runtime::{
    scheduler::{group::TaskGroup, Task, TaskId},
    SharedObject,
};
use ::slab::Slab;
use ::std::ops::{Deref, DerefMut};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Internal offset into the slab that holds the task state.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct InternalId(usize);

#[derive(Default)]
pub struct Scheduler {
    // A list of groups. We just use direct mapping for identifying these because they are never externalized.
    groups: Slab<TaskGroup>,
}

#[derive(Clone, Default)]
pub struct SharedScheduler(SharedObject<Scheduler>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl Scheduler {
    pub fn create_group(&mut self) -> TaskId {
        let internal_id: InternalId = self.groups.insert(TaskGroup::default()).into();
        // Returns an identifier for the group directly derived from the offset.
        Into::<u64>::into(internal_id).into()
    }

    fn get_group(&self, id: TaskId) -> Option<&TaskGroup> {
        // Get the internal id of the parent task or group.
        let group_id: InternalId = Into::<u64>::into(id).into();
        self.groups.get(group_id.into())
    }

    fn get_mut_group(&mut self, id: TaskId) -> Option<&mut TaskGroup> {
        let group_id: InternalId = Into::<u64>::into(id).into();
        self.groups.get_mut(group_id.into())
    }

    /// The parent id can either be the id of the group or another task in the same group.
    pub fn insert_task<T: Task>(&mut self, group_id: TaskId, task: T) -> Option<TaskId> {
        let group: &mut TaskGroup = self.get_mut_group(group_id)?;
        group.insert(Box::new(task))
    }

    #[cfg(test)]
    /// Remove a task from the given group.
    fn remove_task(&mut self, group_id: TaskId, task_id: TaskId) -> Option<Box<dyn Task>> {
        let group: &mut TaskGroup = self.get_mut_group(group_id)?;
        group.remove(task_id)
    }

    pub fn poll_group_once(&mut self, group_id: TaskId) -> Vec<Box<dyn Task>> {
        let mut completed_tasks: Vec<Box<dyn Task>> = vec![];
        // Expect is safe here because something has really gone wrong if we are polling a group that doesn't exist.
        let group: &mut TaskGroup = self.get_mut_group(group_id).expect("group being polled doesn't exist");
        let ready_tasks: Vec<InternalId> = group.get_offsets_for_ready_tasks();
        for id in ready_tasks {
            // Now that we have a runnable task, actually poll it.
            if let Some(task) = group.poll_notified_task_and_remove_if_ready(id) {
                completed_tasks.push(task);
            }
        }
        completed_tasks
    }

    pub fn poll_group_until_unrunnable(&mut self, group_id: TaskId, max_iterations: usize) -> Vec<Box<dyn Task>> {
        let mut completed_tasks: Vec<Box<dyn Task>> = vec![];
        // Keep running as long as there are runnable tasks.
        let mut iterations: usize = 0;
        // Expect is safe here because something has really gone wrong if we are polling a group that doesn't exist.
        let group: &mut TaskGroup = self.get_mut_group(group_id).expect("group being polled doesn't exist");
        while iterations < max_iterations {
            let ready_tasks: Vec<InternalId> = group.get_offsets_for_ready_tasks();
            if ready_tasks.is_empty() {
                break;
            }
            for id in ready_tasks {
                // Now that we have a runnable task, actually poll it.
                if let Some(task) = group.poll_notified_task_and_remove_if_ready(id) {
                    completed_tasks.push(task);
                }
                iterations += 1;
            }
        }
        completed_tasks
    }

    pub fn is_valid_task(&self, group_id: &TaskId, task_id: &TaskId) -> bool {
        if let Some(group) = self.get_group(*group_id) {
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
            scheduler::{Scheduler, TaskId},
            task::TaskWithResult,
        },
    };
    use ::anyhow::Result;
    use ::futures::FutureExt;
    use ::std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use ::test::{black_box, Bencher};

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
        if let Some(task) = scheduler.poll_group_once(group_id).pop() {
            crate::ensure_eq!(task.get_id(), task_id);
        } else {
            anyhow::bail!("task should have completed");
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
        if let Some(task) = scheduler.poll_group_once(group_id).pop() {
            crate::ensure_eq!(task_id, task.get_id());
            Ok(())
        } else {
            anyhow::bail!("task should have completed")
        }
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
        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        let result = scheduler.poll_group_once(group_id).pop();
        crate::ensure_eq!(result.is_some(), false);

        // This shall make the future ready.
        if let Some(task) = scheduler.poll_group_once(group_id).pop() {
            crate::ensure_eq!(task.get_id(), task_id);
        } else {
            anyhow::bail!("task should have completed");
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
        if let Some(task) = scheduler.poll_group_until_unrunnable(group_id, 10).pop() {
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
        let group_id: TaskId = scheduler.create_group();

        // Create and run a task.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(0).fuse()));
        let Some(task_id) = scheduler.insert_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        if let Some(task) = scheduler.poll_group_once(group_id).pop() {
            crate::ensure_eq!(task.get_id(), task_id);
        } else {
            anyhow::bail!("task should have completed");
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
            crate::ensure_eq!(true, scheduler.is_valid_task(&group_id, &task_id));
            scheduler.remove_task(group_id, task_id);
            curr_num_tasks = curr_num_tasks - 1;
            crate::ensure_eq!(scheduler.num_tasks(), curr_num_tasks);
            // The id map does not dictate whether the id is valid, so we need to check the task slab as well.
            crate::ensure_eq!(false, scheduler.is_valid_task(&group_id, &task_id));
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
            black_box(scheduler.poll_group_until_unrunnable(group_id, 100000000));
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
            black_box(scheduler.poll_group_until_unrunnable(group_id, 100000000));
        });
    }
}
