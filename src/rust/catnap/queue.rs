// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::socket::Socket,
    runtime::{
        queue::IoQueue,
        QType,
    },
};
use ::std::os::unix::prelude::RawFd;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata: Catnap control block
pub struct CatnapQueue {
    qtype: QType,
    fd: Option<RawFd>,
    socket: Socket,
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
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatnapQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
