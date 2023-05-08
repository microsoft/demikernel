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
pub struct TcpQueue<const N: usize> {
    socket: Socket<N>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> TcpQueue<N> {
    pub fn new() -> Self {
        Self {
            socket: Socket::Inactive(None),
        }
    }

    /// Get/borrow reference to underlying TCP socket data structure.
    pub fn get_socket(&self) -> &Socket<N> {
        &self.socket
    }

    /// Get/borrow mutable reference to underlying TCP socket data structure.
    pub fn get_mut_socket(&mut self) -> &mut Socket<N> {
        &mut self.socket
    }

    /// Set underlying TCP socket data structure.
    pub fn set_socket(&mut self, s: Socket<N>) {
        self.socket = s;
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl<const N: usize> IoQueue for TcpQueue<N> {
    fn get_qtype(&self) -> QType {
        QType::TcpSocket
    }
}
