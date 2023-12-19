// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    scheduler::TaskHandle,
    types::{
        demi_qresult_t,
        demi_sgarray_t,
    },
    QDesc,
    QToken,
};

#[cfg(feature = "catmem-libos")]
use crate::{
    catmem::SharedCatmemLibOS,
    runtime::memory::MemoryRuntime,
    runtime::SharedDemiRuntime,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Associated functions for Memory LibOSes.
pub enum MemoryLibOS {
    #[cfg(feature = "catmem-libos")]
    Catmem {
        runtime: SharedDemiRuntime,
        libos: SharedCatmemLibOS,
    },
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for memory LibOSes
impl MemoryLibOS {
    /// Creates a memory queue and connects to the consumer/pop-only end.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime: _, libos } => libos.create_pipe(name),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Opens an existing memory queue and connects to the producer/push-only end.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime: _, libos } => libos.open_pipe(name),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Asynchronously closes a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn async_close(&mut self, memqd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime: _, libos } => libos.async_close(memqd),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Pushes a scatter-gather array to a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn push(&mut self, memqd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime: _, libos } => libos.push(memqd, sga),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Pops data from a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn pop(&mut self, memqd: QDesc, size: Option<usize>) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime: _, libos } => libos.pop(memqd, size),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Allocates a scatter-gather array.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime, libos: _ } => runtime.sgaalloc(size),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Releases a scatter-gather array.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime, libos: _ } => runtime.sgafree(sga),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Waits for any operation in an I/O queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn get_task_handle(&mut self, qt: QToken) -> Result<TaskHandle, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime, libos: _ } => runtime.get_task_handle(qt),
            _ => unreachable!("unknown memory libos"),
        }
    }

    #[allow(unreachable_patterns, unused_variables)]
    pub fn pack_result(&mut self, handle: TaskHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime, libos } => runtime.remove_coroutine_and_get_result(&handle, qt.into()),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Waits for any operation in an I/O queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn poll(&mut self) {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem { runtime, libos: _ } => runtime.poll(),
            _ => unreachable!("unknown memory libos"),
        }
    }
}
