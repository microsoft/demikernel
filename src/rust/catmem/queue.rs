// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::pipe::Pipe;
use crate::{
    collections::shared_ring::SharedRingBuffer,
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
    pub fn new(ring: SharedRingBuffer<u16>) -> Self {
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
        let (mut task_handle, _): (TaskHandle, YielderHandle) = self
            .pending_ops
            .remove_entry(handle)
            .expect("operation should be registered");

        // Remove the key so this doesn't cause the scheduler to drop the whole task.
        // We need a better explicit mechanism to remove tasks from the scheduler.
        // FIXME: https://github.com/demikernel/demikernel/issues/593
        task_handle.take_task_id();
    }

    /// Cancels all pending operations on this queue.
    pub fn cancel_pending_ops(&mut self, cause: Fail) {
        for (mut handle, mut yielder_handle) in self.pending_ops.drain() {
            if !handle.has_completed() {
                yielder_handle.wake_with(Err(cause.clone()));
            }
            // Remove the key so this doesn't cause the scheduler to drop the whole task.
            // We need a better explicit mechanism to remove tasks from the scheduler.
            // FIXME: https://github.com/demikernel/demikernel/issues/593
            handle.take_task_id();
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
