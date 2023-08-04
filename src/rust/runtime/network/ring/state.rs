// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::operation::RingControlOperation;
use crate::runtime::fail::Fail;

/// Encodes the state of a ring.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RingState {
    /// A ring that is opened.
    Opened,
    /// A ring that is closing.
    Closing,
    /// A ring that is closed.
    /// For a producer (push) ring, this means that it has successfully set an EOF.
    /// For a consumer (pop) ring, this means that it has received an EOF.
    Closed,
}

/// Drives the state of a ring.
pub struct RingStateMachine {
    /// Current state.
    current: RingState,
    /// Next state.
    next: Option<RingState>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl RingStateMachine {
    /// Constructs [self] with [RingState::Opened] as starting state.
    pub fn new() -> Self {
        Self {
            current: RingState::Opened,
            next: None,
        }
    }

    /// Asserts whether the current state is [RingState::Closed].
    pub fn is_closed(&self) -> bool {
        self.current == RingState::Closed
    }

    /// Prepares to move into the next state.
    pub fn prepare(&mut self, op: RingControlOperation) -> Result<(), Fail> {
        let next: RingState = self.get_next_state(op)?;
        if next != RingState::Closing {
            if self.next.is_some() {
                return Err(fail(op, &(format!("ring is busy")), libc::EBUSY));
            }
        }
        self.next = Some(next);
        Ok(())
    }

    /// Commits to moving into the prepared state.
    pub fn commit(&mut self) {
        self.current = self.next.unwrap_or(self.current);
        self.next = None;
    }

    /// Aborts prepared state.
    pub fn abort(&mut self) {
        self.next = None;
    }

    // Get the next state for this queue.
    fn get_next_state(&mut self, op: RingControlOperation) -> Result<RingState, Fail> {
        match self.current {
            RingState::Opened => self.from_opened(op),
            RingState::Closing => self.from_closing(op),
            RingState::Closed => self.from_closed(op),
        }
    }

    /// Attempts to transition from the [RingState::Opened].
    fn from_opened(&self, op: RingControlOperation) -> Result<RingState, Fail> {
        match op {
            RingControlOperation::Close => Ok(RingState::Closing),
            RingControlOperation::Closed => Err(fail(op, &(format!("ring is closed")), libc::EBADF)),
        }
    }

    /// Attempts to transition from the [RingState::Closing].
    fn from_closing(&self, op: RingControlOperation) -> Result<RingState, Fail> {
        match op {
            RingControlOperation::Close => Ok(RingState::Closing),
            RingControlOperation::Closed => Ok(RingState::Closed),
        }
    }

    /// Attempts to transition from the [RingState::Closed].
    fn from_closed(&self, op: RingControlOperation) -> Result<RingState, Fail> {
        match op {
            RingControlOperation::Close => Err(fail(op, &(format!("ring is closed")), libc::EBADF)),
            RingControlOperation::Closed => Err(fail(op, &(format!("ring is closed")), libc::EBADF)),
        }
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Constructs a [Fail] object from the given `op`, `cause`, and `errno`.
fn fail(op: RingControlOperation, cause: &str, errno: i32) -> Fail {
    error!("{:?}(): {}", op, cause);
    Fail::new(errno, cause)
}
