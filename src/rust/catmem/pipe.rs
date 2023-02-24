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

/// A pipe.
pub struct Pipe {
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

    /// Gets a reference to the underlying buffer of the target pipe.
    pub fn buffer(&self) -> Rc<SharedRingBuffer<u16>> {
        self.buffer.clone()
    }
}
