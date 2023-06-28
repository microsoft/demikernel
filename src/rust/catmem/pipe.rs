// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::collections::shared_ring::SharedRingBuffer;
use ::std::rc::Rc;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Encodes the states of a pipe.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PipeState {
    /// A pipe that is opened.
    Opened,
    /// A pipe that is closing.
    Closing,
    /// A pipe that is closed.
    Closed,
}

/// A pipe.
pub struct Pipe {
    /// State of the pipe.
    state: PipeState,
    /// Indicates end of file.
    eof: bool,
    /// Underlying buffer.
    buffer: Rc<SharedRingBuffer<u16>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Pipe {
    /// Creates a new pipe.
    pub fn new(buffer: SharedRingBuffer<u16>) -> Self {
        Self {
            state: PipeState::Opened,
            eof: false,
            buffer: Rc::new(buffer),
        }
    }

    /// Sets the end of file flag in the target pipe.
    pub fn set_eof(&mut self) {
        self.eof = true;
    }

    /// Gets the value of he end of file flag in the target pipe.
    pub fn eof(&self) -> bool {
        self.eof
    }

    /// Gets the state of the pipe.
    pub fn state(&self) -> PipeState {
        self.state
    }

    /// Sets the state of the pipe.
    pub fn set_state(&mut self, state: PipeState) {
        self.state = state;
    }

    /// Gets a reference to the underlying buffer of the target pipe.
    pub fn buffer(&self) -> Rc<SharedRingBuffer<u16>> {
        self.buffer.clone()
    }
}
