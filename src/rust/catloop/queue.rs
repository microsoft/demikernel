// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    duplex_pipe::DuplexPipe,
    Socket,
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

use ::std::{
    collections::HashMap,
    rc::Rc,
};
//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue meta-data: address and pipe for data transfer
pub struct CatloopQueue {
    qtype: QType,
    socket: Socket,
    pipe: Option<Rc<DuplexPipe>>,
    pending_ops: HashMap<TaskHandle, YielderHandle>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatloopQueue {
    pub fn new(qtype: QType) -> Self {
        Self {
            qtype: qtype,
            socket: Socket::Active(None),
            pipe: None,
            pending_ops: HashMap::<TaskHandle, YielderHandle>::new(),
        }
    }

    /// Get socket type and address associated with this queue.
    pub fn get_socket(&self) -> Socket {
        self.socket
    }

    /// Set socket type and address for this queue.
    pub fn set_socket(&mut self, socket: Socket) {
        self.socket = socket;
    }

    /// Get underlying bi-directional pipe.
    pub fn get_pipe(&self) -> Option<Rc<DuplexPipe>> {
        match &self.pipe {
            Some(pipe) => Some(pipe.clone()),
            None => None,
        }
    }

    /// Set underlying bi-directional pipe.
    pub fn set_pipe(&mut self, pipe: Rc<DuplexPipe>) {
        self.pipe = Some(pipe.clone());
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

impl IoQueue for CatloopQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
