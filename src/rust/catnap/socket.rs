// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::fail::Fail;
use ::std::net::SocketAddrV4;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Encodes the state of a socket.
#[derive(Copy, Clone, Debug)]
enum SocketState {
    /// A socket that is not bound.
    NotBound,
    /// A socket that is bound to a local address.
    Bound,
    /// A socket that is bound to a local address and is able to accept incoming connections.
    Listening,
    /// A socket that is bound to a local address and is accepting incoming connections.
    Accepting,
    /// A socket that is attempting to connect to a remote address.
    Connecting,
    /// A socket that is connected to a remote address.
    Connected,
    /// A socket that is closed.
    Closed,
}

/// A socket.
#[derive(Copy, Clone, Debug)]
pub struct Socket {
    /// The state of the socket.
    state: SocketState,
    /// The local address to which the socket is bound.
    local: Option<SocketAddrV4>,
    /// The remote address to which the socket is connected.
    remote: Option<SocketAddrV4>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Socket {
    /// Creates a new socket that is not bound to an address.
    pub fn new() -> Self {
        Self {
            state: SocketState::NotBound,
            local: None,
            remote: None,
        }
    }

    /// Constructs from [self] a socket that is bound to the `local` address.
    pub fn bind(&self, local: SocketAddrV4) -> Result<Self, Fail> {
        const FN_NAME: &str = "bind";
        match self.state {
            SocketState::NotBound => Ok(Self {
                state: SocketState::Bound,
                local: Some(local),
                remote: None,
            }),
            SocketState::Bound => Err(fail(
                FN_NAME,
                &(format!("socket is bound to address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Listening => Err(fail(
                FN_NAME,
                &(format!("socket is listening on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Accepting => Err(fail(
                FN_NAME,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Connecting => Err(fail(
                FN_NAME,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EINVAL,
            )),
            SocketState::Connected => Err(fail(
                FN_NAME,
                &(format!("socket is connected to address: {:?}", self.remote)),
                libc::EISCONN,
            )),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Constructs from [self] a socket that is able to accept incoming connections.
    pub fn listen(&self) -> Result<Self, Fail> {
        const FN_NAME: &str = "listen";
        match self.state {
            SocketState::NotBound => {
                let cause: String = format!("socket is not bound");
                Err(fail(FN_NAME, &cause, libc::EDESTADDRREQ))
            },
            SocketState::Bound => Ok(Self {
                state: SocketState::Listening,
                local: self.local,
                remote: None,
            }),
            SocketState::Listening => Err(fail(
                FN_NAME,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EADDRINUSE,
            )),
            SocketState::Accepting => Err(fail(
                FN_NAME,
                &(format!("socket is already accepting connections on address: {:?}", self.local)),
                libc::EADDRINUSE,
            )),
            SocketState::Connecting => Err(fail(
                FN_NAME,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EADDRINUSE,
            )),
            SocketState::Connected => Err(fail(
                FN_NAME,
                &(format!("socket is connected to address: {:?}", self.remote)),
                libc::EISCONN,
            )),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Constructs from [self] a socket that is accepting incoming connections.
    pub fn accept(&self) -> Result<Self, Fail> {
        const FN_NAME: &str = "accept";
        match self.state {
            SocketState::NotBound => Err(fail(FN_NAME, &(format!("socket is not bound")), libc::EINVAL)),
            SocketState::Bound => Err(fail(
                FN_NAME,
                &(format!("socket is bound to address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Listening => Ok(Self {
                state: SocketState::Accepting,
                local: self.local,
                remote: None,
            }),
            SocketState::Accepting => Err(fail(
                FN_NAME,
                &(format!("socket is already accepting connections on address: {:?}", self.local)),
                libc::EINPROGRESS,
            )),
            SocketState::Connecting => Err(fail(
                FN_NAME,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EINVAL,
            )),
            SocketState::Connected => Err(fail(
                FN_NAME,
                &(format!("socket is connected to address: {:?}", self.remote)),
                libc::EISCONN,
            )),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Constructs from [self] a socket that has accepted an incoming connection.
    pub fn accepted(&self) -> Result<Self, Fail> {
        const FN_NAME: &str = "accepted";
        match self.state {
            SocketState::NotBound => Err(fail(FN_NAME, &(format!("socket is not bound")), libc::EINVAL)),
            SocketState::Bound => Err(fail(
                FN_NAME,
                &(format!("socket is bound to address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Listening => Err(fail(
                FN_NAME,
                &(format!("socket is listening on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Accepting => Ok(Self {
                state: SocketState::Listening,
                local: self.local,
                remote: self.remote,
            }),
            SocketState::Connecting => Err(fail(
                FN_NAME,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EINVAL,
            )),
            SocketState::Connected => Err(fail(
                FN_NAME,
                &(format!("socket is already connected to address: {:?}", self.remote)),
                libc::EISCONN,
            )),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Constructs from [self] a socket that is attempting to connect to a remote address.
    pub fn connect(&self, remote: SocketAddrV4) -> Result<Self, Fail> {
        const FN_NAME: &str = "connect";
        match self.state {
            SocketState::NotBound | SocketState::Bound => Ok(Self {
                state: SocketState::Connecting,
                local: self.local,
                remote: Some(remote),
            }),
            SocketState::Listening => Err(fail(
                FN_NAME,
                &(format!("socket is listening on address: {:?}", self.local)),
                libc::EOPNOTSUPP,
            )),
            SocketState::Accepting => Err(fail(
                FN_NAME,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EOPNOTSUPP,
            )),
            SocketState::Connecting => Err(fail(
                FN_NAME,
                &(format!("socket is already connecting to address: {:?}", self.remote)),
                libc::EINPROGRESS,
            )),
            SocketState::Connected => Err(fail(
                FN_NAME,
                &(format!("socket is already connected to address: {:?}", self.remote)),
                libc::EISCONN,
            )),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Constructs from [self] a socket that is connected to the `remote` address.
    pub fn connected(&self, remote: SocketAddrV4) -> Result<Self, Fail> {
        const FN_NAME: &str = "connected";
        match self.state {
            SocketState::NotBound => Ok(Self {
                state: SocketState::Connected,
                local: self.local,
                remote: Some(remote),
            }),
            SocketState::Bound => Err(fail(
                FN_NAME,
                &(format!("socket is bound to address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Listening => Err(fail(
                FN_NAME,
                &(format!("socket is listening on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketState::Accepting => Err(fail(
                FN_NAME,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EBUSY,
            )),
            SocketState::Connecting => Ok(Self {
                state: SocketState::Connected,
                local: self.local,
                remote: Some(remote),
            }),
            SocketState::Connected => Err(fail(
                FN_NAME,
                &(format!("socket is already connected to address: {:?}", self.remote)),
                libc::EISCONN,
            )),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Constructs from [self] a socket that is closed.
    pub fn close(&self) -> Result<Self, Fail> {
        const FN_NAME: &str = "close";
        match self.state {
            SocketState::NotBound
            | SocketState::Bound
            | SocketState::Listening
            | SocketState::Accepting
            | SocketState::Connecting
            | SocketState::Connected => Ok(Self {
                state: SocketState::Closed,
                local: self.local,
                remote: self.remote,
            }),
            SocketState::Closed => Err(fail(FN_NAME, &(format!("socket is closed")), libc::EBADF)),
        }
    }

    /// Returns the `local` address to which [self] is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    /// Returns the `remote` address tot which [self] is connected.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Constructs a [Fail] object from the given `fn_name`, `cause`, and `errno`.
fn fail(fn_name: &str, cause: &str, errno: i32) -> Fail {
    error!("{}(): {}", fn_name, cause);
    Fail::new(errno, cause)
}
