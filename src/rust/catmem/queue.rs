// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    pipe::Pipe,
    CatmemRingBuffer,
};
use crate::{
    runtime::{
        fail::Fail,
        queue::IoQueue,
        QType,
    },
    scheduler::{
        TaskHandle,
        YielderHandle,
    },
};
use ::std::collections::HashMap;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata: reference to the shared pipe.
pub struct CatmemQueue {
    pipe: Pipe,
    pending_ops: HashMap<TaskHandle, YielderHandle>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatmemQueue {
    pub fn new(ring: CatmemRingBuffer) -> Self {
        Self {
            pipe: Pipe::new(ring),
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        }
    }

    /// Get underlying uni-directional pipe.
    pub fn get_pipe(&self) -> &Pipe {
        &self.pipe
    }

    /// Set underlying uni-directional pipe.
    pub fn get_mut_pipe(&mut self) -> &mut Pipe {
        &mut self.pipe
    }

    /// Adds a new operation to the list of pending operations on this queue.
    pub fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops.insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue.
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops
            .remove_entry(handle)
            .expect("operation should be registered");
    }

    /// Cancels all pending operations on this queue.
    pub fn cancel_pending_ops(&mut self, cause: Fail) {
        for (handle, mut yielder_handle) in self.pending_ops.drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
        }
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatmemQueue {
    fn get_qtype(&self) -> QType {
        QType::MemoryQueue
    }
}
