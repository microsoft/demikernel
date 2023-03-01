// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use super::pipe::Pipe;
use crate::{
    collections::shared_ring::SharedRingBuffer,
    runtime::{
        queue::IoQueue,
        QType,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Per-queue metadata: reference to the shared pipe.
pub struct CatmemQueue {
    pipe: Pipe,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl CatmemQueue {
    pub fn new(ring: SharedRingBuffer<u16>) -> Self {
        Self { pipe: Pipe::new(ring) }
    }

    /// Get underlying uni-directional pipe.
    pub fn get_pipe(&self) -> &Pipe {
        &self.pipe
    }

    /// Set underlying uni-directional pipe.
    pub fn get_mut_pipe(&mut self) -> &mut Pipe {
        &mut self.pipe
    }
}

//======================================================================================================================
// Trait implementation
//======================================================================================================================

impl IoQueue for CatmemQueue {
    fn get_qtype(&self) -> QType {
        QType::MemoryQueue
    }
}
