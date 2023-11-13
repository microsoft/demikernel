// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
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
    pub fn unlock(&self) -> Result<(), Fail> {
        // Unlock this mutex. Must return true as it was previously locked.
        let locked: bool = self.locked.borrow_mut().swap(false, Ordering::Release);
        match locked {
            true => Ok(()),
            false => {
                let cause: String = format!("mutex was not locked");
                error!("unlock(): {}", &cause);
                Err(Fail::new(libc::EPERM, &cause))
            },
        }
    }
}

//======================================================================================================================
// Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use ::anyhow::Result;

    #[test]
    fn test_mutex_acquire_release() -> Result<()> {
        let mutex: Mutex = Mutex::new();

        // Try to acquire and release the lock.
        crate::ensure_eq!(mutex.try_lock(), true);
        crate::ensure_eq!(mutex.unlock().is_ok(), true);

        Ok(())
    }

    #[test]
    fn test_mutex_release_without_acquire() -> Result<()> {
        let mutex: Mutex = Mutex::new();

        // Try to release the lock without acquiring it.
        crate::ensure_eq!(mutex.unlock().is_err(), true);

        Ok(())
    }

    #[test]
    fn test_mutex_aquire_aquire_release() -> Result<()> {
        let mutex: Mutex = Mutex::new();

        // Try to acquire and release the lock.
        crate::ensure_eq!(mutex.try_lock(), true);
        crate::ensure_eq!(mutex.try_lock(), false);
        crate::ensure_eq!(mutex.unlock().is_ok(), true);

        Ok(())
    }

    #[test]
    fn test_mutex_aquire_release_release() -> Result<()> {
        let mutex: Mutex = Mutex::new();

        // Try to acquire and release the lock.
        crate::ensure_eq!(mutex.try_lock(), true);
        crate::ensure_eq!(mutex.unlock().is_ok(), true);
        crate::ensure_eq!(mutex.unlock().is_err(), true);

        Ok(())
    }
}
