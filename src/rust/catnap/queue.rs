// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::socket::Socket,
    runtime::{
        fail::Fail,
        queue::{
            IoQueue,
            QType,
        },
    },
    scheduler::{
        TaskHandle,
        YielderHandle,
    },
};
use ::std::{
    collections::HashMap,
    os::unix::prelude::RawFd,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata: Catnap control block
pub struct CatnapQueue {
    qtype: QType,
    fd: Option<RawFd>,
    socket: Socket,
    pending_ops: HashMap<TaskHandle, YielderHandle>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatnapQueue {
    pub fn new(qtype: QType, fd: Option<RawFd>) -> Self {
        Self {
            qtype,
            fd,
            socket: Socket::new(),
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        }
    }

    /// Get underlying POSIX file descriptor.
    pub fn get_fd(&self) -> Option<RawFd> {
        self.fd
    }

    /// Set underlying POSIX file descriptor.
    pub fn set_fd(&mut self, fd: RawFd) {
        self.fd = Some(fd);
    }

    /// Sets the underlying socket.
    pub fn set_socket(&mut self, socket: &Socket) {
        self.socket = socket.clone();
    }

    /// Gets an immutable references to the underlying socket.
    pub fn get_socket(&self) -> &Socket {
        &self.socket
    }

    /// Adds a new operation to the list of pending operations on this queue.
    pub fn add_pending_op(&mut self, handle: &TaskHandle, yielder_handle: &YielderHandle) {
        self.pending_ops.insert(handle.clone(), yielder_handle.clone());
    }

    /// Removes an operation from the list of pending operations on this queue. This function should only be called if
    /// add_pending_op() was previously called.
    pub fn remove_pending_op(&mut self, handle: &TaskHandle) {
        self.pending_ops.remove(handle);
    }

    /// Cancel all currently pending operations on this queue. If the operation is not complete and the coroutine has
    /// yielded, wake the coroutine with an error.
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

impl IoQueue for CatnapQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
