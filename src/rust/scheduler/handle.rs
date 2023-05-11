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
    /// External identifying token.
    task_id: Option<u64>,
    /// Corresponding location in the scheduler's memory chunk.
    index: Option<usize>,
    /// Memory chunk in which the corresponding handle lives.
    chunk: WakerPageRef,
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

/// Associate Functions for Scheduler Handlers
impl TaskHandle {
    /// Creates a new Scheduler Handle.
    pub fn new(task_id: u64, index: usize, waker_page: WakerPageRef) -> Self {
        Self {
            index: Some(index),
            task_id: Some(task_id),
            chunk: waker_page,
        }
    }

    /// Takes out the key stored in the target [SchedulerHandle].
    pub fn take_task_id(&mut self) -> Option<u64> {
        self.index.take();
        self.task_id.take()
    }

    /// Queries whether or not the future associated with the target [SchedulerHandle] has completed.
    pub fn has_completed(&self) -> bool {
        let subpage_ix: usize = self.index.unwrap() as usize & (WAKER_BIT_LENGTH - 1);
        self.chunk.has_completed(subpage_ix)
    }

    /// Returns the raw key stored in the target [SchedulerHandle].
    pub fn get_task_id(mut self) -> u64 {
        self.index.take();
        self.task_id.take().unwrap()
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

/// Drop Trait Implementation for Scheduler Handlers
impl Drop for TaskHandle {
    /// Decreases the reference count of the target [SchedulerHandle].
    fn drop(&mut self) {
        if let Some(key) = self.index.take() {
            let subpage_ix: usize = key as usize & (WAKER_BIT_LENGTH - 1);
            self.chunk.mark_dropped(subpage_ix);
        }
    }
}

impl Hash for TaskHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let key: u64 = self
            .task_id
            .expect("SchedulerHandle should have a key to insert into hashmap");
        key.hash(state);
    }
}

impl PartialEq for TaskHandle {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}
impl Eq for TaskHandle {}
