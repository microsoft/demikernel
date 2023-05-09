// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::{
        fail::Fail,
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
    },
    scheduler::TaskHandle,
};

#[cfg(feature = "catmem-libos")]
use crate::catmem::CatmemLibOS;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Associated functions for Memory LibOSes.
pub enum MemoryLibOS {
    #[cfg(feature = "catmem-libos")]
    Catmem(CatmemLibOS),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for memory LibOSes
impl MemoryLibOS {
    /// Creates a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.create_pipe(name),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Opens an existing memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.open_pipe(name),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Closes a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn close(&mut self, memqd: QDesc) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.close(memqd),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Pushes a scatter-gather array to a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn push(&mut self, memqd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.push(memqd, sga),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Pops data from a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn pop(&mut self, memqd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.pop(memqd, size),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Allocates a scatter-gather array.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.alloc_sgarray(size),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Releases a scatter-gather array.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.free_sgarray(sga),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Waits for any operation in an I/O queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn schedule(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.schedule(qt),
            _ => unreachable!("unknown memory libos"),
        }
    }

    #[allow(unreachable_patterns, unused_variables)]
    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.pack_result(handle, qt),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Waits for any operation in an I/O queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn poll(&mut self) {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.poll(),
            _ => unreachable!("unknown memory libos"),
        }
    }
}
