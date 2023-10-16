// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::peer::Socket;
use crate::runtime::{
    queue::IoQueue,
    QType,
    SharedObject,
};
use ::std::{
    any::Any,
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata for the TCP socket.
pub struct TcpQueue<const N: usize> {
    socket: Socket<N>,
}

#[derive(Clone)]
pub struct SharedTcpQueue<const N: usize>(SharedObject<TcpQueue<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> SharedTcpQueue<N> {
    /// Create a new shared queue.
    pub fn new() -> Self {
        Self(SharedObject::<TcpQueue<N>>::new(TcpQueue {
            socket: Socket::Inactive(None),
        }))
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

impl<const N: usize> IoQueue for SharedTcpQueue<N> {
    fn get_qtype(&self) -> QType {
        QType::TcpSocket
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

impl<const N: usize> Deref for SharedTcpQueue<N> {
    type Target = TcpQueue<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedTcpQueue<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
