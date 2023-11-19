// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    network::socket::operation::SocketOp,
};
use ::socket2::Type;

//======================================================================================================================
// Structures
//======================================================================================================================

/// States of a Socket.
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

/// Encodes the state of a socket.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SocketStateMachine {
    typ: Type,
    previous: Option<SocketState>,
    current: SocketState,
    next: Option<SocketState>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SocketStateMachine {
    /// Constructs a new [SocketState] of type `typ` that is on unbound state.
    pub fn new_unbound(typ: Type) -> Self {
        // This was previously checked in the LibOS layer.
        debug_assert!(typ == Type::STREAM || typ == Type::DGRAM);
        Self {
            typ,
            previous: None,
            current: SocketState::NotBound,
            next: None,
        }
    }

    /// Constructs a new [SocketState] that is on connected state.
    pub fn new_connected() -> Self {
        Self {
            typ: Type::STREAM,
            previous: None,
            current: SocketState::Connected,
            next: None,
        }
    }

    /// Asserts whether the target may continue accepting connections.
    pub fn may_accept(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;
        self.ensure_accepting()?;
        Ok(())
    }

    /// Asserts whether the target may continue connecting to a remote.
    pub fn may_connect(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;
        Ok(())
    }

    /// Asserts whether the target [SocketState] may push data.
    pub fn may_push(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;

        if self.typ == Type::STREAM {
            self.ensure_connected()?;
        }

        // NOTE: no need to ensure other states, because this is checked on the prepare operation.

        Ok(())
    }

    /// Asserts whether the target [SocketState] may pop data.
    pub fn may_pop(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;

        if self.typ == Type::STREAM {
            self.ensure_connected()?;
        } else {
            self.ensure_bound()?;
        }

        // NOTE: no need to ensure other states, because this is checked on the prepare operation.

        Ok(())
    }

    /// Commits to moving into the prepared state
    pub fn commit(&mut self) {
        self.previous = Some(self.current);
        self.current = self.next.unwrap_or(self.current);
        self.next = None;
    }

    /// Rolls back the prepared state.
    pub fn abort(&mut self) {
        self.next = None;
    }

    /// Rollback to previous state.
    pub fn rollback(&mut self) {
        self.abort();
        self.current = self.previous.unwrap_or(self.current);
    }

    /// Prepares to move into the next state.
    pub fn prepare(&mut self, op: SocketOp) -> Result<(), Fail> {
        let next: SocketState = self.get_next_state(op)?;
        if next != SocketState::Closing {
            if let Some(pending) = self.next {
                if pending != next {
                    return Err(fail(op, &(format!("socket is busy")), libc::EBUSY));
                }
            }
        }
        self.next = Some(next);
        Ok(())
    }

    /// Given the current state and the operation being executed, this function returns the next state on success and
    fn get_next_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        let next_state: Result<SocketState, Fail> = match self.current {
            SocketState::NotBound => self.not_bound_state(op),
            SocketState::Bound => self.bound_state(op),
            SocketState::Listening => self.listening_state(op),
            SocketState::Accepting => self.accepting_state(op),
            SocketState::Connecting => self.connecting_state(op),
            SocketState::Connected => self.connected_state(op),
            SocketState::Closing => self.closing_state(op),
            SocketState::Closed => self.closed_state(op),
        };
        match next_state {
            Ok(state) => {
                if state != self.current && self.next != Some(state) {
                    debug!(
                        "get_next_state(): previous={:?}, current={:?}, transition={:?}",
                        self.previous, self.current, op
                    );
                }
                Ok(state)
            },
            Err(e) => {
                warn!(
                    "not valid transition: previous={:?}, current={:?}, transition={:?}, error={:?}",
                    self.previous, self.current, op, e
                );
                Err(e)
            },
        }
    }

    /// Attempts to transition from a not bound state.
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

    /// Attempts to transition from a bound state.
    fn bound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accept | SocketOp::Accepted | SocketOp::Connected => {
                Err(fail(op, &(format!("socket is already bound")), libc::EINVAL))
            },
            SocketOp::Listen => Ok(SocketState::Listening),
            SocketOp::Connect => Ok(SocketState::Connecting),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a listening state.
    fn listening_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accepted | SocketOp::Connected => {
                Err(fail(op, &(format!("socket is already listening")), libc::EINVAL))
            },
            SocketOp::Listen => Err(fail(op, &(format!("socket is already listening")), libc::EADDRINUSE)),
            SocketOp::Accept => Ok(SocketState::Accepting),
            SocketOp::Connect => Err(fail(op, &(format!("socket is already listening")), libc::EOPNOTSUPP)),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from an accepting state.
    fn accepting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Err(fail(
                op,
                &(format!("socket is already accepting connections")),
                libc::EINVAL,
            )),
            SocketOp::Listen => Err(fail(
                op,
                &(format!("socket is already accepting connections")),
                libc::EADDRINUSE,
            )),
            SocketOp::Accept => Err(fail(
                op,
                &(format!("socket is already accepting connections")),
                libc::EINPROGRESS,
            )),
            SocketOp::Accepted => Ok(SocketState::Listening),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket is already accepting connection")),
                libc::ENOTSUP,
            )),
            // Should this be possible without going through the Connecting state?
            SocketOp::Connected => Err(fail(
                op,
                &(format!("socket is already accepting connections")),
                libc::EBUSY,
            )),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a `Connecting` state.
    fn connecting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Accept | SocketOp::Accepted => {
                Err(fail(op, &(format!("socket is already connecting")), libc::EINVAL))
            },
            SocketOp::Listen => Err(fail(op, &(format!("socket is already connecting")), libc::EADDRINUSE)),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket already is already connecting ")),
                libc::EINPROGRESS,
            )),
            SocketOp::Connected => Ok(SocketState::Connected),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a connected state.
    fn connected_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            // Does this make sense if we didn't go through the Connecting state?
            SocketOp::Bind => Ok(SocketState::Connected),
            SocketOp::Listen | SocketOp::Accept | SocketOp::Accepted | SocketOp::Connect | SocketOp::Connected => {
                Err(fail(op, &(format!("socket is already connected")), libc::EISCONN))
            },
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a closing state.
    fn closing_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        if op == SocketOp::Closed {
            Ok(SocketState::Closed)
        } else {
            Err(fail(op, &(format!("socket is closing")), libc::EBADF))
        }
    }

    /// Attempts to transition from a closed state.
    fn closed_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        Err(fail(op, &(format!("socket is closed")), libc::EBADF))
    }

    /// Ensures that the target [SocketState] is bound.
    fn ensure_bound(&self) -> Result<(), Fail> {
        if self.current != SocketState::Bound {
            let cause: String = format!("socket is not bound");
            error!("ensure_bound(): {}", cause);
            return Err(Fail::new(libc::EDESTADDRREQ, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is accepting incoming connections.
    fn ensure_accepting(&self) -> Result<(), Fail> {
        if self.current != SocketState::Accepting {
            let cause: String = format!("socket is not listening");
            error!("ensure_listening(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is connected.
    fn ensure_connected(&self) -> Result<(), Fail> {
        if self.current != SocketState::Connected {
            let cause: String = format!("socket is not connected");
            error!("ensure_connected(): {}", cause);
            return Err(Fail::new(libc::ENOTCONN, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is not closing.
    fn ensure_not_closing(&self) -> Result<(), Fail> {
        if self.current == SocketState::Closing {
            let cause: String = format!("socket is closing");
            error!("ensure_not_closing(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is not closed.
    fn ensure_not_closed(&self) -> Result<(), Fail> {
        if self.current == SocketState::Closed {
            let cause: String = format!("socket is closed");
            error!("ensure_not_closed(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }
        Ok(())
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Constructs a [Fail] object from the given `op`, `cause`, and `errno`.
fn fail(op: SocketOp, cause: &str, errno: i32) -> Fail {
    error!("{:?}(): {}", op, cause);
    Fail::new(errno, cause)
}
