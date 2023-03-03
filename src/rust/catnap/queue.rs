// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    queue::IoQueue,
    QType,
};
use ::std::os::unix::prelude::RawFd;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata: Catnap control block
pub struct CatnapQueue {
    qtype: QType,
    fd: Option<RawFd>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatnapQueue {
    pub fn new(qtype: QType, fd: Option<RawFd>) -> Self {
        Self { qtype, fd }
    }

    /// Get underlying POSIX file descriptor.
    pub fn get_fd(&self) -> Option<RawFd> {
        self.fd
    }

    /// Set underlying POSIX file descriptor.
    pub fn set_fd(&mut self, fd: RawFd) {
        self.fd = Some(fd);
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
