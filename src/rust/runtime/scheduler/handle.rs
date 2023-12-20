// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::fail::Fail;
use ::std::{
    cell::RefCell,
    rc::Rc,
    task::Waker,
};

//==============================================================================
// Structures
//==============================================================================

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
        } else if let Some(waker) = self.waker_handle.borrow_mut().take() {
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
