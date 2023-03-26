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

/// Scheduler Handle
///
/// This is used to uniquely identify a future in the scheduler.
#[derive(Clone)]
pub struct SchedulerHandle {
    /// Corresponding location in the scheduler's memory chunk.
    key: Option<u64>,
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
impl SchedulerHandle {
    /// Creates a new Scheduler Handle.
    pub fn new(key: u64, waker_page: WakerPageRef) -> Self {
        Self {
            key: Some(key),
            chunk: waker_page,
        }
    }

    /// Takes out the key stored in the target [SchedulerHandle].
    pub fn take_key(&mut self) -> Option<u64> {
        self.key.take()
    }

    /// Queries whether or not the future associated with the target [SchedulerHandle] has completed.
    pub fn has_completed(&self) -> bool {
        let subpage_ix: usize = self.key.unwrap() as usize & (WAKER_BIT_LENGTH - 1);
        self.chunk.has_completed(subpage_ix)
    }

    /// Returns the raw key stored in the target [SchedulerHandle].
    pub fn into_raw(mut self) -> u64 {
        self.key.take().unwrap()
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
        if let Some(res) = self.result_handle.borrow_mut().replace(result) {
            debug!(
                "YielderHandle::wake_with() already scheduled: overwriting previous wake result: {:?}",
                res
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
impl Drop for SchedulerHandle {
    /// Decreases the reference count of the target [SchedulerHandle].
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let subpage_ix: usize = key as usize & (WAKER_BIT_LENGTH - 1);
            self.chunk.mark_dropped(subpage_ix);
        }
    }
}

impl Hash for SchedulerHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let key: u64 = self
            .key
            .expect("SchedulerHandle should have a key to insert into hashmap");
        key.hash(state);
    }
}

impl PartialEq for SchedulerHandle {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for SchedulerHandle {}
