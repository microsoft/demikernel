// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::peer::Socket;
use crate::runtime::{
    queue::IoQueue,
    QType,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata for the TCP socket.
pub struct TcpQueue {
    socket: Socket,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl TcpQueue {
    pub fn new() -> Self {
        Self {
            socket: Socket::Inactive(None),
        }
    }

    /// Get/borrow reference to underlying TCP socket data structure.
    pub fn get_socket(&self) -> &Socket {
        &self.socket
    }

    /// Get/borrow mutable reference to underlying TCP socket data structure.
    pub fn get_mut_socket(&mut self) -> &mut Socket {
        &mut self.socket
    }

    /// Set underlying TCP socket data structure.
    pub fn set_socket(&mut self, s: Socket) {
        self.socket = s;
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for TcpQueue {
    fn get_qtype(&self) -> QType {
        QType::TcpSocket
    }
}
