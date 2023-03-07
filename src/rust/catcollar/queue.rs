// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    queue::IoQueue,
    QType,
};
use ::std::{
    net::SocketAddrV4,
    os::unix::prelude::RawFd,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Catcollar control block: meta data stored per queue.
#[derive(Copy, Clone)]
pub struct CatcollarQueue {
    qtype: QType,
    fd: Option<RawFd>,
    addr: Option<SocketAddrV4>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatcollarQueue {
    /// Creates a new metadata structure for a queue.
    pub fn new(qtype: QType) -> Self {
        Self {
            qtype: qtype,
            fd: None,
            addr: None,
        }
    }

    /// Get the underlying Linux raw socket.
    pub fn get_fd(&self) -> Option<RawFd> {
        self.fd
    }

    /// Set the underlying Linux raw socket.
    pub fn set_fd(&mut self, fd: RawFd) {
        self.fd = Some(fd);
    }

    /// Gets underlying socket address.
    pub fn get_addr(&self) -> Option<SocketAddrV4> {
        self.addr
    }

    /// Sets underlying socket address.
    pub fn set_addr(&mut self, addr: SocketAddrV4) {
        self.addr = Some(addr);
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatcollarQueue {
    fn get_qtype(&self) -> QType {
        self.qtype
    }
}
