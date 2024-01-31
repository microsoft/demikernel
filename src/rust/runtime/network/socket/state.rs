// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_value::SharedAsyncValue,
    runtime::{
        fail::Fail,
        network::socket::operation::SocketOp,
    },
};
use ::socket2::Type;
use ::std::time::Duration;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Set the timeout to be large enough that we effectively never time out.
const TIMEOUT: Duration = Duration::from_secs(1000);

//======================================================================================================================
// Structures
//======================================================================================================================

/// States of a Socket.
#[derive(Copy, Clone, Debug, PartialEq)]
enum SocketState {
    /// A socket that is not bound.
    Unbound,
    /// A socket that is bound to a local address.
    Bound,
    /// A socket that is bound to a local address and is able to accept incoming connections.
    PassiveListening,
    /// A socket that is connecting to a remote address.
    ActiveConnecting,
    /// A socket that is connected to a remote address.
    ActiveEstablished,
    /// A socket that is closing.
    Closing,
    /// A socket that is closed.
    Closed,
}

/// Encodes the state of a socket.
#[derive(Clone, Debug)]
pub struct SocketStateMachine {
    typ: Type,
    current: SharedAsyncValue<SocketState>,
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
            current: SharedAsyncValue::new(SocketState::Unbound),
            next: None,
        }
    }

    /// Constructs a new [SocketState] for an established connection.
    pub fn new_established() -> Self {
        Self {
            typ: Type::STREAM,
            current: SharedAsyncValue::new(SocketState::ActiveEstablished),
            next: None,
        }
    }

    /// Asserts whether the target may continue accepting connections.
    pub fn may_accept(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;
        self.ensure_listening()?;
        Ok(())
    }

    // Blocks until the socket is no longer in a state where it may accept.
    pub async fn while_may_accept(&mut self) -> Fail {
        loop {
            match self.may_accept() {
                Ok(()) => {
                    // If either a time out or the current state changed, check it again.
                    _ = self.current.clone().wait_for_change(Some(TIMEOUT)).await;
                    continue;
                },
                Err(e) => return e,
            }
        }
    }

    /// Asserts whether the target may continue connecting to a remote.
    pub fn may_connect(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;
        Ok(())
    }

    // Blocks until the socket is no longer in a state where it may connect.
    pub async fn while_may_connect(&mut self) -> Fail {
        loop {
            match self.may_connect() {
                Ok(()) => {
                    // If either a time out or the current state changed, check it again.
                    _ = self.current.clone().wait_for_change(Some(TIMEOUT)).await;
                    continue;
                },
                Err(e) => return e,
            }
        }
    }

    /// Asserts whether the target [SocketState] may push data.
    pub fn may_push(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;

        if self.typ == Type::STREAM {
            self.ensure_established()?;
        }

        // NOTE: no need to ensure other states, because this is checked on the prepare operation.

        Ok(())
    }

    // Blocks until the socket is no longer in a state where it may connect.
    pub async fn while_may_push(&mut self) -> Fail {
        loop {
            match self.may_push() {
                Ok(()) => {
                    // If either a time out or the current state changed, check it again.
                    _ = self.current.clone().wait_for_change(Some(TIMEOUT)).await;
                    continue;
                },
                Err(e) => return e,
            }
        }
    }

    /// Asserts whether the target [SocketState] may pop data.
    pub fn may_pop(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;

        if self.typ == Type::STREAM {
            self.ensure_established()?;
        } else {
            self.ensure_bound()?;
        }

        // NOTE: no need to ensure other states, because this is checked on the prepare operation.

        Ok(())
    }

    // Blocks until the socket is no longer in a state where it may connect.
    pub async fn while_may_pop(&mut self) -> Fail {
        loop {
            match self.may_pop() {
                Ok(()) => {
                    // If either a time out or the current state changed, check it again.
                    _ = self.current.clone().wait_for_change(Some(TIMEOUT)).await;
                    continue;
                },
                Err(e) => return e,
            }
        }
    }

    /// Commits to moving into the prepared state
    pub fn commit(&mut self) {
        let current: SocketState = self.current.get();
        self.current.set(self.next.unwrap_or(current));
        self.next = None;
    }

    /// Rolls back the prepared state.
    pub fn abort(&mut self) {
        self.next = None;
    }

    /// Prepares to move into the next state.
    pub fn prepare(&mut self, op: SocketOp) -> Result<(), Fail> {
        let next: SocketState = self.get_next_state(op)?;

        // We have already prepared and not committed or aborted yet.
        if let Some(pending) = self.next {
            if next != SocketState::Closing && pending != next {
                return Err(fail(op, &(format!("socket is busy")), libc::EBUSY));
            }
        }

        self.next = Some(next);
        Ok(())
    }

    /// Given the current state and the operation being executed, this function returns the next state on success and
    fn get_next_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        let next_state: Result<SocketState, Fail> = match self.current.get() {
            SocketState::Unbound => self.unbound_state(op),
            SocketState::Bound => self.bound_state(op),
            SocketState::PassiveListening => self.listening_state(op),
            SocketState::ActiveConnecting => self.connecting_state(op),
            SocketState::ActiveEstablished => self.established_state(op),
            SocketState::Closing => self.closing_state(op),
            SocketState::Closed => self.closed_state(op),
        };
        match next_state {
            Ok(state) => {
                if state != self.current.get() && self.next != Some(state) {
                    debug!("get_next_state(): current={:?}, transition={:?}", self.current, op);
                }
                Ok(state)
            },
            Err(e) => {
                warn!(
                    "not valid transition: current={:?}, transition={:?}, error={:?}",
                    self.current, op, e
                );
                Err(e)
            },
        }
    }

    /// Attempts to transition from a not bound state.
    fn unbound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Ok(SocketState::Bound),
            SocketOp::Listen => Err(fail(op, &format!("socket is not bound"), libc::EDESTADDRREQ)),
            SocketOp::Connect => Ok(SocketState::ActiveConnecting),
            // Should this be possible without going through the Connecting state?
            SocketOp::Established => Ok(SocketState::ActiveEstablished),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a bound state.
    fn bound_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Err(fail(op, &(format!("socket is already bound")), libc::EINVAL)),
            SocketOp::Listen => Ok(SocketState::PassiveListening),
            SocketOp::Connect => Ok(SocketState::ActiveConnecting),
            SocketOp::Established => Ok(SocketState::ActiveConnecting),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a listening state.
    fn listening_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Established => {
                Err(fail(op, &(format!("socket is already listening")), libc::EINVAL))
            },
            SocketOp::Listen => Err(fail(op, &(format!("socket is already listening")), libc::EADDRINUSE)),
            SocketOp::Connect => Err(fail(op, &(format!("socket is already listening")), libc::EOPNOTSUPP)),
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Err(fail(op, &(format!("socket is busy")), libc::EBUSY)),
        }
    }

    /// Attempts to transition from a `Connecting` state.
    fn connecting_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind => Err(fail(op, &(format!("socket is already connecting")), libc::EINVAL)),
            SocketOp::Listen => Err(fail(op, &(format!("socket is already connecting")), libc::EADDRINUSE)),
            SocketOp::Connect => Err(fail(
                op,
                &(format!("socket already is already connecting ")),
                libc::EINPROGRESS,
            )),
            SocketOp::Established => Ok(SocketState::ActiveEstablished),
            SocketOp::Close => Ok(SocketState::Closing),
            // We may enter the closed state from other states because either the state machine was incorrectly rolled
            // back or the close cased another operation to fail.
            // FIXME: https://github.com/microsoft/demikernel/issues/1035
            SocketOp::Closed => Ok(SocketState::Closed),
        }
    }

    /// Attempts to transition from a connected state.
    fn established_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Bind | SocketOp::Listen | SocketOp::Connect | SocketOp::Established => {
                Err(fail(op, &(format!("socket is already connected")), libc::EISCONN))
            },
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Ok(SocketState::Closed),
        }
    }

    /// Attempts to transition from a closing state.
    fn closing_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        match op {
            SocketOp::Close => Ok(SocketState::Closing),
            SocketOp::Closed => Ok(SocketState::Closed),
            _ => Err(fail(op, &(format!("socket is closing")), libc::EBADF)),
        }
    }

    /// Attempts to transition from a closed state.
    fn closed_state(&self, op: SocketOp) -> Result<SocketState, Fail> {
        if op == SocketOp::Closed || op == SocketOp::Close {
            Ok(SocketState::Closed)
        } else {
            Err(fail(op, &(format!("socket is closed")), libc::EBADF))
        }
    }

    /// Ensures that the target [SocketState] is bound.
    fn ensure_bound(&self) -> Result<(), Fail> {
        if self.current.get() != SocketState::Bound {
            let cause: String = format!("socket is not bound");
            error!("ensure_bound(): {}", cause);
            return Err(Fail::new(libc::EDESTADDRREQ, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is accepting incoming connections.
    fn ensure_listening(&self) -> Result<(), Fail> {
        if self.current.get() != SocketState::PassiveListening {
            let cause: String = format!("socket is not listening");
            error!("ensure_listening(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is connected.
    fn ensure_established(&self) -> Result<(), Fail> {
        if self.current.get() != SocketState::ActiveEstablished {
            let cause: String = format!("socket is not connected");
            error!("ensure_connected(): {}", cause);
            return Err(Fail::new(libc::ENOTCONN, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is not closing.
    fn ensure_not_closing(&self) -> Result<(), Fail> {
        if self.current.get() == SocketState::Closing {
            let cause: String = format!("socket is closing");
            error!("ensure_not_closing(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [SocketState] is not closed.
    fn ensure_not_closed(&self) -> Result<(), Fail> {
        if self.current.get() == SocketState::Closed {
            let cause: String = format!("socket is closed");
            error!("ensure_not_closed(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }
        Ok(())
    }

    fn get_state(&self) -> SocketState {
        self.current.get()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl PartialEq for SocketStateMachine {
    fn eq(&self, other: &Self) -> bool {
        self.get_state() == other.get_state()
    }

    fn ne(&self, other: &Self) -> bool {
        self.get_state() != other.get_state()
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
