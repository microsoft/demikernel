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
#[derive(Copy, Clone, Debug, PartialEq)]
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
    /// A socket that is closing.
    Closing,
    /// A socket that is closed.
    Closed,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum SocketOp {
    Bind,
    Listen,
    Accept,
    Accepted,
    Connect,
    Connected,
    Close,
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
        match self.get_next_state(SocketOp::Bind) {
            Ok(state) => Ok(Self {
                state,
                local: Some(local),
                remote: None,
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that is able to accept incoming connections.
    pub fn listen(&self) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Listen) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: None,
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that is accepting incoming connections.
    pub fn accept(&self) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Accept) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: None,
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that has accepted an incoming connection.
    pub fn accepted(&self) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Accepted) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: None,
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that is attempting to connect to a remote address.
    pub fn connect(&self, remote: SocketAddrV4) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Connect) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: Some(remote),
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that is connected to the `remote` address.
    pub fn connected(&self, remote: SocketAddrV4) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Connected) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: Some(remote),
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that is closing.
    pub fn close(&self) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Close) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: self.remote,
            }),
            Err(e) => Err(e),
        }
    }

    /// Constructs from [self] a socket that is closed.
    pub fn closed(&self) -> Result<Self, Fail> {
        match self.get_next_state(SocketOp::Closed) {
            Ok(state) => Ok(Self {
                state,
                local: self.local,
                remote: self.remote,
            }),
            Err(e) => Err(e),
        }
    }

    /// Returns the `local` address to which [self] is bound.
    pub fn local(&self) -> Option<SocketAddrV4> {
        self.local
    }

    #[allow(dead_code)]
    /// Returns the `remote` address tot which [self] is connected.
    pub fn remote(&self) -> Option<SocketAddrV4> {
        self.remote
    }

    /// Asserts if [self] is `Connecting`.
    pub fn is_connecting(&self) -> bool {
        self.state == SocketState::Connecting
    }

    /// Given the current state and the operation being executed, this function returns the next state on success and
    fn get_next_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        debug!("state: {:?} transition: {:?}", self.state, op);
        match self.state {
            SocketState::NotBound => self.not_bound_state(op),
            SocketState::Bound => self.bound_state(op),
            SocketState::Listening => self.listening_state(op),
            SocketState::Accepting => self.accepting_state(op),
            SocketState::Connecting => self.connecting_state(op),
            SocketState::Connected => self.connected_state(op),
            SocketState::Closing => self.closing_state(op),
            SocketState::Closed => self.closed_state(op),
        }
    }

    fn not_bound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Ok(SocketState::Bound),
            SocketOp::Listen => Err(fail(op, &format!("socket is not bound"), libc::EDESTADDRREQ)),
            SocketOp::Accept | SocketOp::Accepted => Err(fail(op, &(format!("socket is not bound")), libc::EINVAL)),
            SocketOp::Connect => Ok(SocketState::Connecting),
            // Should this be possible without going through the Connecting state?
            SocketOp::Connected => Ok(SocketState::Connected),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn bound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accept | SocketOp::Accepted | SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is already bound to address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Ok(SocketState::Listening),
            SocketOp::Connect => Ok(SocketState::Connecting),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn listening_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accepted | SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EADDRINUSE,
            )),
            SocketOp::Accept => Ok(SocketState::Accepting),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket is already listening on address: {:?}", self.local)),
                libc::EOPNOTSUPP,
            )),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn accepting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EADDRINUSE,
            )),
            SocketOp::Accept => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EINPROGRESS,
            )),
            SocketOp::Accepted => Ok(SocketState::Listening),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::ENOTSUP,
            )),
            // Should this be possible without going through the Connecting state?
            SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is accepting connections on address: {:?}", self.local)),
                libc::EBUSY,
            )),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn connecting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accept | SocketOp::Accepted => Err(fail(
                op,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is connecting to address: {:?}", self.remote)),
                libc::EADDRINUSE,
            )),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket already is connecting to address: {:?}", self.remote)),
                libc::EINPROGRESS,
            )),
            SocketOp::Connected => Ok(SocketState::Connected),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn connected_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            // Does this make sense if we didn't go through the Connecting state?
            SocketOp::Bind => Ok(SocketState::Connected),
            SocketOp::Listen | SocketOp::Accept | SocketOp::Accepted | SocketOp::Connect | SocketOp::Connected => {
                Err(fail(
                    op,
                    &(format!("socket is already connected to address: {:?}", self.remote)),
                    libc::EISCONN,
                ))
            },
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    fn closing_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        if op == SocketOp::Closed {
            Ok(SocketState::Closed)
        } else {
            Err(fail(op, &(format!("socket is closing")), libc::EBADF))
        }
    }

    fn closed_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        Err(fail(op, &(format!("socket is closed")), libc::EBADF))
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Constructs a [Fail] object from the given `fn_name`, `cause`, and `errno`.
fn fail(op: SocketOp, cause: &str, errno: i32) -> Fail {
    error!("{:?}(): {}", op, cause);
    Fail::new(errno, cause)
}
