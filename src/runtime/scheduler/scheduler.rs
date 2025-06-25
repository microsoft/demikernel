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
    scheduler::{group::TaskGroup, Task},
    SharedObject,
};
use ::slab::Slab;
use ::std::{
    ops::{Deref, DerefMut},
    pin::Pin,
};

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Default)]
pub struct Scheduler {
    // A list of groups. We just use direct mapping for identifying these because they are never externalized.
    groups: Slab<TaskGroup>,
}

/// Internal ids used by the scheduler. These should NEVER be exposed to the application without first going through
/// the runtime.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct SchedulerId(pub usize);

#[derive(Clone, Default)]
pub struct SharedScheduler(SharedObject<Scheduler>);

/// Possible outcomes for inserting a task.
#[derive(Debug)]
pub enum InsertResult {
    Inserted(SchedulerId),
    Completed(Box<dyn Task>),
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl Scheduler {
    pub fn create_group(&mut self) -> SchedulerId {
        self.groups.insert(TaskGroup::default()).into()
    }

    fn get_mut_group(&mut self, id: SchedulerId) -> Option<&mut TaskGroup> {
        self.groups.get_mut(id.into())
    }

    /// The parent id can either be the id of the group or another task in the same group.
    pub fn insert_and_poll_task<T: Task>(&mut self, group_id: SchedulerId, task: T) -> Option<InsertResult> {
        let group: &mut TaskGroup = self.get_mut_group(group_id)?;
        group.insert_and_poll_task(Box::new(task))
    }

    pub fn get_mut_task(&mut self, group_id: SchedulerId, task_id: SchedulerId) -> Option<Pin<&mut Box<dyn Task>>> {
        let group: &mut TaskGroup = self.get_mut_group(group_id)?;
        group.get_mut_task(task_id.into())
    }

    /// Polls all ready tasks in this group until there are no runnable ones. Do not use this function on coroutines
    /// that use poll_yield, unless max_iterations is set.
    pub fn poll_group_until_unrunnable(
        &mut self,
        group_id: SchedulerId,
        max_iterations: Option<usize>,
    ) -> Vec<Box<dyn Task>> {
        // Expect is safe here because something has really gone wrong if we are polling a group that doesn't exist.
        let group: &mut TaskGroup = self.get_mut_group(group_id).expect("group being polled doesn't exist");

        // Keep polling the group and checking for runnable tasks until there are none left.
        group.poll_group(max_iterations, true)
    }

    /// Polls all of the ready tasks in this group. Only check for new tasks at the beginning.
    pub fn poll_group_once(&mut self, group_id: SchedulerId, max_iterations: Option<usize>) -> Vec<Box<dyn Task>> {
        // Expect is safe here because something has really gone wrong if we are polling a group that doesn't exist.
        let group: &mut TaskGroup = self.get_mut_group(group_id).expect("group being polled doesn't exist");
        group.poll_group(max_iterations, false)
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

impl From<usize> for SchedulerId {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<SchedulerId> for usize {
    fn from(value: SchedulerId) -> Self {
        value.0
    }
}

impl From<u64> for SchedulerId {
    fn from(value: u64) -> Self {
        Self(value as usize)
    }
}

impl From<SchedulerId> for u64 {
    fn from(value: SchedulerId) -> Self {
        value.0 as u64
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::runtime::scheduler::{
        scheduler::{InsertResult, Scheduler, SchedulerId},
        task::TaskWithResult,
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
            match self.as_ref().val {
                0 => Poll::Ready(()),
                _ => {
                    self.get_mut().val -= 1;
                    let waker: &Waker = ctx.waker();
                    waker.wake_by_ref();
                    Poll::Pending
                },
            }
        }
    }

    type DummyTask = TaskWithResult<()>;

    #[test]
    fn poll_group_with_one_small_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: SchedulerId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(1).fuse()));
        let Some(InsertResult::Inserted(_)) = scheduler.insert_and_poll_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, our future should complete.
        if let Some(_) = scheduler.poll_group_once(group_id, None).pop() {
            Ok(())
        } else {
            anyhow::bail!("task should have completed")
        }
    }

    #[test]
    fn poll_group_twice_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: SchedulerId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete
        // with two poll operations.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(2).fuse()));
        let Some(InsertResult::Inserted(_)) = scheduler.insert_and_poll_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling once, this future should make a transition.
        let result = scheduler.poll_group_once(group_id, None).pop();
        crate::ensure_eq!(result.is_some(), false);

        // This shall make the future ready.
        if let Some(_) = scheduler.poll_group_once(group_id, None).pop() {
            Ok(())
        } else {
            anyhow::bail!("task should have completed");
        }
    }

    #[test]
    fn poll_until_unrunnable_with_one_long_task_completes_it() -> Result<()> {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: SchedulerId = scheduler.create_group();

        // Insert a single future in the scheduler. This future shall complete with a single poll operation.
        let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(1).fuse()));
        let Some(InsertResult::Inserted(_)) = scheduler.insert_and_poll_task(group_id, task) else {
            anyhow::bail!("insert() failed")
        };

        // All futures are inserted in the scheduler with notification flag set.
        // By polling until the task completes, our future should complete.
        if let Some(_) = scheduler.poll_group_until_unrunnable(group_id, None).pop() {
            Ok(())
        } else {
            anyhow::bail!("task should have completed")
        }
    }

    #[bench]
    fn insert_bench(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: SchedulerId = scheduler.create_group();

        b.iter(|| {
            let task: DummyTask = DummyTask::new("testing", Box::pin(black_box(DummyCoroutine::new(1).fuse())));
            let Some(InsertResult::Inserted(task_id)) = scheduler.insert_and_poll_task(group_id, task) else {
                panic!("couldn't insert future in scheduler");
            };
            black_box(task_id);
        });
    }

    #[bench]
    fn benchmark_poll_one_task_at_a_time(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: SchedulerId = scheduler.create_group();

        const NUM_TASKS: usize = 1024;
        let mut task_ids: Vec<SchedulerId> = Vec::<SchedulerId>::with_capacity(NUM_TASKS);

        for val in 1..NUM_TASKS {
            let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(val).fuse()));
            match scheduler.insert_and_poll_task(group_id, task) {
                Some(InsertResult::Completed(_)) => panic!("task should not have completed"),
                Some(InsertResult::Inserted(task_id)) => task_ids.push(task_id),
                None => panic!("insert() failed"),
            };
        }

        b.iter(|| {
            black_box(scheduler.poll_group_once(group_id, None));
        });
    }

    #[bench]
    fn poll_many_tasks_until_done_bench(b: &mut Bencher) {
        let mut scheduler: Scheduler = Scheduler::default();
        let group_id: SchedulerId = scheduler.create_group();

        const NUM_TASKS: usize = 8;
        let mut task_ids: Vec<SchedulerId> = Vec::<SchedulerId>::with_capacity(NUM_TASKS);

        for val in 1..NUM_TASKS {
            let task: DummyTask = DummyTask::new("testing", Box::pin(DummyCoroutine::new(val).fuse()));
            match scheduler.insert_and_poll_task(group_id, task) {
                Some(InsertResult::Completed(_)) => panic!("task should not have completed"),
                Some(InsertResult::Inserted(task_id)) => task_ids.push(task_id),
                None => panic!("insert() failed"),
            };
        }

        b.iter(|| {
            black_box(scheduler.poll_group_until_unrunnable(group_id, None));
        });
    }
}
