// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::fail::Fail,
    scheduler::yielder::Yielder,
};

use ::std::{
    cell::{
        Ref,
        RefCell,
    },
    rc::Rc,
    sync::atomic::{
        AtomicBool,
        Ordering,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Mutex for ensuring single-threaded access to a resource.
#[derive(Clone)]
pub struct Mutex {
    locked: Rc<RefCell<AtomicBool>>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================
impl Mutex {
    pub fn new() -> Self {
        Self {
            locked: Rc::new(RefCell::<AtomicBool>::new(AtomicBool::new(false))),
        }
    }

    /// Acquire this lock. If the lock is locked, then yield once and try again until we are able to acquire the lock.
    pub async fn lock(&self, yielder: Yielder) -> Result<(), Fail> {
        let mut locked: Ref<AtomicBool> = self.locked.borrow();
        while locked.swap(true, Ordering::Acquire) {
            drop(locked);
            // Could not acquire lock. Yield and try again.
            // TODO: Make this more efficient by signaling.
            match yielder.yield_once().await {
                Ok(()) => locked = self.locked.borrow(),
                Err(cause) => return Err(cause),
            }
        }
        Ok(())
    }

    /// Try to acquire this lock. Return [true] if successful
    pub fn try_lock(&self) -> bool {
        let locked: Ref<AtomicBool> = self.locked.borrow();
        match locked.swap(true, Ordering::Acquire) {
            // The lock was previously locked.
            true => false,
            false => true,
        }
    }

    /// Release this lock.
    pub fn unlock(&self) {
        // Unlock this mutex. Must return true as it was previously locked.
        assert_eq!(self.locked.borrow_mut().swap(false, Ordering::Release), true);
    }
}
