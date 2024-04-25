// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    network::ring::operation::RingControlOperation,
};

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

    /// Asserts wether the target [RingState] is in a state were we push data.
    pub fn may_push(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;
        Ok(())
    }

    /// Asserts wether the target [RingState] is in a state were we pop data.
    pub fn may_pop(&self) -> Result<(), Fail> {
        self.ensure_not_closing()?;
        self.ensure_not_closed()?;
        Ok(())
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

    /// Ensures that the target [RingState] is not [RingState::Closing].
    fn ensure_not_closing(&self) -> Result<(), Fail> {
        if self.current == RingState::Closing {
            let cause: String = format!("ring is closing");
            error!("ensure_not_closing(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }
        Ok(())
    }

    /// Ensures that the target [RingState] is not [RingState::Closed].
    fn ensure_not_closed(&self) -> Result<(), Fail> {
        if self.current == RingState::Closed {
            let cause: String = format!("ring is closed");
            error!("ensure_not_clossed(): {}", cause);
            return Err(Fail::new(libc::EBADF, &cause));
        }
        Ok(())
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
