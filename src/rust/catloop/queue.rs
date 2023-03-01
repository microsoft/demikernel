// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::{
    duplex_pipe::DuplexPipe,
    Socket,
};
use crate::runtime::{
    queue::IoQueue,
    QType,
};
use ::std::rc::Rc;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue meta-data: address and pipe for data transfer
pub struct CatloopQueue {
    qtype: QType,
    socket: Socket,
    pipe: Option<Rc<DuplexPipe>>,
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
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatloopQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
