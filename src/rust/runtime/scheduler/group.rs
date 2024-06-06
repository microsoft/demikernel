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
    collections::{
        id_map::IdMap,
        pin_slab::PinSlab,
    },
    expect_some,
    runtime::scheduler::{
        page::{
            WakerPageRef,
            WakerRef,
        },
        scheduler::InternalId,
        waker64::{
            WAKER_BIT_LENGTH,
            WAKER_BIT_LENGTH_SHIFT,
        },
        Task,
        TaskId,
    },
};
use ::bit_iter::BitIter;
use ::futures::Future;
use ::std::{
    collections::VecDeque,
    pin::Pin,
    ptr::NonNull,
    task::{
        Context,
        Poll,
        Waker,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This represents a resource management group. All tasks belong to a task group. By default, a task belongs to the
/// same task group as the allocating task.
#[derive(Default)]
pub struct TaskGroup {
    ids: IdMap<TaskId, InternalId>,
    /// Stores all the tasks that are held by the scheduler.
    tasks: PinSlab<Box<dyn Task>>,
    /// Holds the waker bits for controlling task scheduling.
    waker_page_refs: Vec<WakerPageRef>,
    // The current set of ready tasks in the group.
    ready_tasks: VecDeque<InternalId>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl TaskGroup {
    /// Given a handle to a task, remove it from the scheduler
    pub fn remove(&mut self, task_id: TaskId) -> Option<Box<dyn Task>> {
        // We should not have a scheduler handle that refers to an invalid id, so unwrap and expect are safe here.
        let pin_slab_index: usize =
            expect_some!(self.ids.remove(&task_id), "Token should be in the token table").into();
        let (waker_page_ref, waker_page_offset): (&WakerPageRef, usize) = {
            let (waker_page_index, waker_page_offset) = self.get_waker_page_index_and_offset(pin_slab_index)?;
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

    /// Insert a new task into our scheduler returning a handle corresponding to it.
    pub fn insert(&mut self, task_id: TaskId, task: Box<dyn Task>) -> bool {
        let task_name: &'static str = task.get_name();
        // The pin slab index can be reverse-computed in a page index and an offset within the page.
        let pin_slab_index: usize = match self.tasks.insert(task) {
            Some(index) => index,
            None => return false,
        };
        self.ids.insert(task_id, pin_slab_index.into());

        self.add_new_pages_up_to_pin_slab_index(pin_slab_index.into());

        // Initialize the appropriate page offset.
        let (waker_page_ref, waker_page_offset): (&WakerPageRef, usize) = {
            let (waker_page_index, waker_page_offset) = match self.get_waker_page_index_and_offset(pin_slab_index) {
                Some(result) => result,
                None => return false,
            };
            (&self.waker_page_refs[waker_page_index], waker_page_offset)
        };
        waker_page_ref.initialize(waker_page_offset);

        trace!(
            "insert(): name={:?}, id={:?}, pin_slab_index={:?}",
            task_name,
            task_id,
            pin_slab_index
        );
        // Set this task's id. Expect is safe here because we just allocated the
        expect_some!(self.tasks.get_pin_mut(pin_slab_index), "just allocated!").set_id(task_id);
        true
    }

    /// Computes the page and page offset of a given task based on its total offset.
    fn get_waker_page_index_and_offset(&self, pin_slab_index: usize) -> Option<(usize, usize)> {
        // This check ensures that the slab slot is actually occupied but trusts that the pin_slab_index is for this
        // task.
        if !self.tasks.contains(pin_slab_index) {
            return None;
        }
        let waker_page_index: usize = pin_slab_index >> WAKER_BIT_LENGTH_SHIFT;
        let waker_page_offset: usize = Self::get_waker_page_offset(pin_slab_index);
        Some((waker_page_index, waker_page_offset))
    }

    /// Add new page(s) to hold this future's status if the current page is filled. This may result in addition of
    /// multiple pages because of the gap between the pin slab index and the current page index.
    fn add_new_pages_up_to_pin_slab_index(&mut self, pin_slab_index: usize) {
        while pin_slab_index >= (self.waker_page_refs.len() << WAKER_BIT_LENGTH_SHIFT) {
            self.waker_page_refs.push(WakerPageRef::default());
        }
    }

    pub fn get_num_waker_pages(&self) -> usize {
        self.waker_page_refs.len()
    }

    fn get_waker_page_offset(pin_slab_index: usize) -> usize {
        pin_slab_index & (WAKER_BIT_LENGTH - 1)
    }

    fn get_pin_slab_index(waker_page_index: usize, waker_page_offset: usize) -> usize {
        (waker_page_index << WAKER_BIT_LENGTH_SHIFT) + waker_page_offset
    }

    pub fn update_offsets_for_ready_tasks(&mut self) {
        for i in 0..self.get_num_waker_pages() {
            // Grab notified bits.
            let notified: u64 = self.waker_page_refs[i].take_notified();
            // Turn into bit iter.
            let mut offset: VecDeque<InternalId> = BitIter::from(notified)
                .map(|x| Self::get_pin_slab_index(i, x).into())
                .collect();
            self.ready_tasks.append(&mut offset);
        }
    }

    /// Translates an internal task id to an external one. Expects the task to exist.
    fn unchecked_internal_to_external_id(&self, internal_id: InternalId) -> TaskId {
        expect_some!(self.tasks.get(internal_id.into()), "Invalid offset: {:?}", internal_id).get_id()
    }

    fn get_pinned_task_ptr(&mut self, pin_slab_index: usize) -> Pin<&mut Box<dyn Task>> {
        // Get the pinned ref.
        expect_some!(
            self.tasks.get_pin_mut(pin_slab_index),
            "Invalid offset: {:?}",
            pin_slab_index
        )
    }

    pub fn get_waker(&self, internal_task_id: InternalId) -> Option<Waker> {
        let (waker_page_index, waker_page_offset) = self.get_waker_page_index_and_offset(internal_task_id.into())?;

        let raw_waker: NonNull<u8> = self.waker_page_refs[waker_page_index].into_raw_waker_ref(waker_page_offset);
        Some(unsafe { Waker::from_raw(WakerRef::new(raw_waker).into()) })
    }

    /// Poll a single task. This function polls the task regardless of if it is ready.
    pub fn poll_task(&mut self, task_id: TaskId) -> Option<Box<dyn Task>> {
        // Safe to expect here because we must have found the task_id to find the group id.
        let internal_id: InternalId = self.ids.get(&task_id).expect("Task should exist");
        // Grab the waker page and assume that it is set
        let (waker_page_ref, waker_page_offset): (&WakerPageRef, usize) = {
            let (waker_page_index, waker_page_offset) = self.get_waker_page_index_and_offset(internal_id.into())?;
            (&self.waker_page_refs[waker_page_index], waker_page_offset)
        };
        waker_page_ref.clear(waker_page_offset);
        self.poll_notified_task_and_remove_if_ready(internal_id)
    }

    /// Does a single sweep of ready bits and runs all ready tasks.
    pub fn poll_all(&mut self) -> Vec<Box<dyn Task>> {
        let mut completed_tasks: Vec<Box<dyn Task>> = vec![];
        // Grab all ready tasks.
        self.update_offsets_for_ready_tasks();

        while let Some(task_id) = self.ready_tasks.pop_front() {
            if let Some(task) = self.poll_notified_task_and_remove_if_ready(task_id) {
                completed_tasks.push(task);
            }
        }
        completed_tasks
    }

    /// Runs coroutines in this group until a completed one is found. Returns the number of coroutines that were polled.
    /// and a completed task if one completed.
    pub fn get_next_completed_task(&mut self, max_iterations: usize) -> (usize, Option<Box<dyn Task>>) {
        for i in 0..max_iterations {
            let task_id: InternalId = match self.ready_tasks.pop_front() {
                Some(id) => id,
                None => {
                    self.update_offsets_for_ready_tasks();
                    if let Some(id) = self.ready_tasks.pop_front() {
                        id
                    } else {
                        return (i, None);
                    }
                },
            };
            trace!("polling task: {:?}", task_id);

            if let Some(task) = self.poll_notified_task_and_remove_if_ready(task_id) {
                return (i + 1, Some(task));
            }
        }
        (max_iterations, None)
    }

    pub fn poll_notified_task_and_remove_if_ready(&mut self, internal_task_id: InternalId) -> Option<Box<dyn Task>> {
        // Perform the actual work of running the task.
        let poll_result: Poll<()> = {
            // Get the waker context.
            let waker: Waker = self.get_waker(internal_task_id)?;
            let mut waker_context: Context = Context::from_waker(&waker);

            let mut pinned_ptr = self.get_pinned_task_ptr(internal_task_id.into());
            let pinned_ref = unsafe { Pin::new_unchecked(&mut *pinned_ptr) };

            // Poll future.
            Future::poll(pinned_ref, &mut waker_context)
        };

        if let Poll::Ready(()) = poll_result {
            let task_id: TaskId = self.unchecked_internal_to_external_id(internal_task_id);
            return self.remove(task_id);
        }
        None
    }

    pub fn is_valid_task(&self, task_id: &TaskId) -> bool {
        if let Some(internal_id) = self.ids.get(task_id) {
            self.tasks.contains(internal_id.into())
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn num_tasks(&self) -> usize {
        self.ids.len()
    }
}
