// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    catnap::transport::{
        active_socket::ActiveSocketData,
        passive_socket::PassiveSocketData,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        SharedObject,
    },
};
use ::socket2::Socket;
use ::std::{
    convert::{
        AsMut,
        AsRef,
    },
    net::SocketAddr,
    ops::{
        Deref,
        DerefMut,
    },
    os::fd::{
        AsRawFd,
        RawFd,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// This structure represents the metadata for a socket.
pub enum SocketData {
    Inactive(Option<Socket>),
    Passive(PassiveSocketData),
    Active(ActiveSocketData),
}

#[derive(Clone)]
/// Shared socket metadata across coroutines.
pub struct SharedSocketData(SharedObject<SocketData>);

//======================================================================================================================
// Implementations
//======================================================================================================================

impl SharedSocketData {
    /// Creates new metadata representing a socket.
    pub fn new_inactive(socket: Socket) -> Self {
        Self(SharedObject::<SocketData>::new(SocketData::Inactive(Some(socket))))
    }

    /// Creates new metadata representing a socket.
    pub fn new_active(socket: Socket) -> Self {
        Self(SharedObject::<SocketData>::new(SocketData::Active(
            ActiveSocketData::new(socket),
        )))
    }

    /// Moves an inactive socket to a passive listening socket.
    pub fn move_socket_to_passive(&mut self) {
        let socket: Socket = match self.deref_mut() {
            SocketData::Inactive(socket) => socket.take().expect("should have data"),
            SocketData::Active(_) => unreachable!("should not be able to move an active socket to a passive one"),
            SocketData::Passive(_) => return,
        };
        self.set_socket_data(SocketData::Passive(PassiveSocketData::new(socket)))
    }

    /// Moves an inactive socket to an active established socket.
    pub fn move_socket_to_active(&mut self) {
        let socket: Socket = match self.deref_mut() {
            SocketData::Inactive(socket) => socket.take().expect("should have data"),
            SocketData::Active(_) => return,
            SocketData::Passive(_) => unreachable!("should not be able to move a passive socket to an active one"),
        };
        self.set_socket_data(SocketData::Active(ActiveSocketData::new(socket)));
    }

    /// Gets a reference to the actual Socket for reading the socket's metadata (mostly the raw file descriptor).
    pub fn get_socket<'a>(&'a self) -> &'a Socket {
        let _self: &'a SocketData = self.as_ref();
        match _self {
            SocketData::Inactive(Some(socket)) => socket,
            SocketData::Active(data) => data.get_socket(),
            SocketData::Passive(data) => data.get_socket(),
            _ => panic!("Should have data"),
        }
    }

    /// Gets a mutable reference to the actual Socket for I/O operations.
    pub fn get_mut_socket<'a>(&'a mut self) -> &'a mut Socket {
        let _self: &'a mut SocketData = self.as_mut();
        match _self {
            SocketData::Inactive(Some(socket)) => socket,
            SocketData::Active(data) => data.get_mut_socket(),
            SocketData::Passive(data) => data.get_mut_socket(),
            _ => panic!("Should have data"),
        }
    }

    /// An internal function for moving sockets between states.
    fn set_socket_data(&mut self, data: SocketData) {
        *self.deref_mut() = data;
    }

    /// Push some data to an active established connection.
    pub async fn push(&mut self, addr: Option<SocketAddr>, buf: DemiBuffer) -> Result<(), Fail> {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("Cannot write to an inactive socket"),
            SocketData::Active(data) => data.push(addr, buf).await,
            SocketData::Passive(_) => unreachable!("Cannot write to a passive socket"),
        }
    }

    /// Accept a new connection on an passive listening socket.
    pub async fn accept(&mut self) -> Result<(Socket, SocketAddr), Fail> {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("Cannot accept on an inactive socket"),
            SocketData::Active(_) => unreachable!("Cannot accept on an active socket"),
            SocketData::Passive(data) => data.accept().await,
        }
    }

    /// Pop some data on an active established connection.
    pub async fn pop(&mut self, buf: &mut DemiBuffer, size: usize) -> Result<Option<SocketAddr>, Fail> {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("Cannot read on an inactive socket"),
            SocketData::Active(data) => data.pop(buf, size).await,
            SocketData::Passive(_) => unreachable!("Cannot read on a passive socket"),
        }
    }

    /// Handle incoming data event.
    pub fn poll_in(&mut self) {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("should only receive incoming events on active or passive sockets"),
            SocketData::Active(data) => data.poll_recv(),
            SocketData::Passive(data) => data.poll_accept(),
        }
    }

    /// Handle an outgoing data event.
    pub fn poll_out(&mut self) {
        match self.deref_mut() {
            SocketData::Inactive(_) => unreachable!("should only receive outgoing events on active or passive sockets"),
            SocketData::Active(data) => data.poll_send(),
            // Nothing to do for passive sockets.
            SocketData::Passive(_) => (),
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Turn a shared socket metadata structure into the raw file descriptor.
impl AsRawFd for SharedSocketData {
    fn as_raw_fd(&self) -> RawFd {
        self.get_socket().as_raw_fd()
    }
}

/// Dereference a shared reference to socket metadata.
impl Deref for SharedSocketData {
    type Target = SocketData;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// Dereference a shared reference to socket metadata.
impl DerefMut for SharedSocketData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Turn a shared reference for socket metadata into a direct reference to the metadata.
impl AsRef<SocketData> for SharedSocketData {
    fn as_ref(&self) -> &SocketData {
        self.0.as_ref()
    }
}

/// Turn a shared mutable reference for socket metadata into a direct mutable reference to the metadata.
impl AsMut<SocketData> for SharedSocketData {
    fn as_mut(&mut self) -> &mut SocketData {
        self.0.as_mut()
    }
}
