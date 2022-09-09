// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::scheduler::{
    page::WakerPageRef,
    waker64::WAKER_BIT_LENGTH,
};

//==============================================================================
// Structures
//==============================================================================

/// Scheduler Handler
///
/// This is used to uniquely identify a future in the scheduler.
pub struct SchedulerHandle {
    /// Corresponding location in the scheduler's memory chunk.
    key: Option<u64>,
    /// Memory chunk in which the corresponding handle lives.
    chunk: WakerPageRef,
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

    /// Queries whether or not the future associated with the target [SchedulerHandle] has complemented.
    pub fn has_completed(&self) -> bool {
        let subpage_ix: usize = self.key.unwrap() as usize & (WAKER_BIT_LENGTH - 1);
        self.chunk.has_completed(subpage_ix)
    }

    /// Returns the raw key stored in the target [SchedulerHandle].
    pub fn into_raw(mut self) -> u64 {
        self.key.take().unwrap()
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
