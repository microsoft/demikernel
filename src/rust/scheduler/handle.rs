// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    runtime::fail::Fail,
    scheduler::{
        page::WakerPageRef,
        waker64::WAKER_BIT_LENGTH,
    },
};
use ::std::{
    cell::RefCell,
    hash::{
        Hash,
        Hasher,
    },
    rc::Rc,
    task::Waker,
};

//==============================================================================
// Structures
//==============================================================================

/// Task Handle
///
/// This is used to uniquely identify a Task in the scheduler. Used to check on the status of the coroutine.
#[derive(Clone)]
pub struct TaskHandle {
    /// External identifier for this task.
    task_id: u64,
    /// Index of this task's status bits in the waker pages.
    pin_slab_index: usize,
    /// Reference to this task's status bits.
    waker_page_ref: WakerPageRef,
}

/// Yield Handle
///
/// This is used to unique identify a yielded coroutine / Task. Used to wake the yielded coroutine.
#[derive(Clone)]
pub struct YielderHandle {
    result_handle: Rc<RefCell<Option<Result<(), Fail>>>>,
    waker_handle: Rc<RefCell<Option<Waker>>>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Task Handlers
impl TaskHandle {
    /// Creates a new Task Handle.
    pub fn new(task_id: u64, pin_slab_index: usize, waker_page_ref: WakerPageRef) -> Self {
        Self { task_id, pin_slab_index, waker_page_ref }
    }

    /// Queries whether or not the coroutine in the Task has completed.
    pub fn has_completed(&self) -> bool {
        let waker_page_offset: usize = self.pin_slab_index & (WAKER_BIT_LENGTH - 1);
        self.waker_page_ref.has_completed(waker_page_offset)
    }

    /// Returns the task_id stored in the target [SchedulerHandle].
    pub fn get_task_id(&self) -> u64 {
        self.task_id
    }

    /// Removes the task from the scheduler and keeps it from running again.
    #[deprecated]
    pub fn deschedule(&mut self) {
        let waker_page_offset: usize = self.pin_slab_index & (WAKER_BIT_LENGTH - 1);
        self.waker_page_ref.mark_dropped(waker_page_offset);
    }
}

impl YielderHandle {
    pub fn new() -> Self {
        Self {
            result_handle: Rc::new(RefCell::new(None)),
            waker_handle: Rc::new(RefCell::new(None)),
        }
    }

    /// Wake this yielded coroutine: Ok indicates there is work to be done and Fail indicates the coroutine should exit
    /// with an error.
    pub fn wake_with(&mut self, result: Result<(), Fail>) {
        if let Some(old_result) = self.result_handle.borrow_mut().replace(result) {
            debug!(
                "wake_with(): already scheduled, overwriting result (old={:?})",
                old_result
            );
        }

        if let Some(waker) = self.waker_handle.borrow_mut().take() {
            waker.wake();
        }
    }

    /// Get the result this coroutine should be woken with.
    pub fn get_result(&mut self) -> Option<Result<(), Fail>> {
        self.result_handle.borrow_mut().take()
    }

    /// Set the waker for this Yielder and return a reference to it.
    pub fn set_waker(&mut self, waker: Waker) {
        *self.waker_handle.borrow_mut() = Some(waker);
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Hash for TaskHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.task_id.hash(state);
    }
}

impl PartialEq for TaskHandle {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}
impl Eq for TaskHandle {}
