// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    runtime::{
        memory::MemoryRuntime,
        Runtime,
    },
    scheduler::scheduler::Scheduler,
};

//==============================================================================
// Structures
//==============================================================================

/// POSIX Runtime
#[derive(Clone)]
pub struct PosixRuntime {
    /// Scheduler
    pub scheduler: Scheduler,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for POSIX Runtime
impl PosixRuntime {
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::default(),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for PosixRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for PosixRuntime {}
