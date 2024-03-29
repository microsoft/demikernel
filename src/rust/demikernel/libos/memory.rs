// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    types::{
        demi_qresult_t,
        demi_sgarray_t,
    },
    QDesc,
    QToken,
};
use ::std::time::Duration;

#[cfg(feature = "catmem-libos")]
use crate::catmem::SharedCatmemLibOS;
#[cfg(feature = "catmem-libos")]
use crate::runtime::memory::MemoryRuntime;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Associated functions for Memory LibOSes.
pub enum MemoryLibOS {
    #[cfg(feature = "catmem-libos")]
    Catmem(SharedCatmemLibOS),
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
            MemoryLibOS::Catmem(libos) => libos.create_pipe(name),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Opens an existing memory queue and connects to the producer/push-only end.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.open_pipe(name),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Asynchronously closes a memory queue.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn async_close(&mut self, memqd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.async_close(memqd),
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

    /// Waits for a pending I/O operation to complete or a timeout to expire.
    /// This is just a single-token convenience wrapper for wait_any().
    pub fn wait(&mut self, qt: QToken, timeout: Duration) -> Result<demi_qresult_t, Fail> {
        trace!("wait(): qt={:?}, timeout={:?}", qt, timeout);

        // Put the QToken into a single element array.
        let qt_array: [QToken; 1] = [qt];

        // Call wait_any() to do the real work.
        let (offset, qr): (usize, demi_qresult_t) = self.wait_any(&qt_array, timeout)?;
        debug_assert_eq!(offset, 0);
        Ok(qr)
    }

    /// Waits for any of the given pending I/O operations to complete or a timeout to expire.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn wait_any(&mut self, qts: &[QToken], timeout: Duration) -> Result<(usize, demi_qresult_t), Fail> {
        trace!("wait_any(): qts={:?}, timeout={:?}", qts, timeout);
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.wait_any(qts, timeout),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Waits in a loop until the next task is complete, passing the result to `acceptor`. This process continues until
    /// either the acceptor returns false (in which case the method returns Ok), or the timeout has expired (in which
    /// the method returns an `Err` indicating timeout).
    #[allow(unreachable_patterns, unused_variables)]
    pub fn wait_next_n<Acceptor: FnMut(demi_qresult_t) -> bool>(
        &mut self,
        acceptor: Acceptor,
        timeout: Duration
    ) -> Result<(), Fail>
    {
        trace!("wait_next_n(): acceptor, timeout={:?}", timeout);
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.wait_next_n(acceptor, timeout),
            _ => unreachable!("unknown memory libos"),
        }
    }


    /// Allocates a scatter-gather array.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.sgaalloc(size),
            _ => unreachable!("unknown memory libos"),
        }
    }

    /// Releases a scatter-gather array.
    #[allow(unreachable_patterns, unused_variables)]
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catmem-libos")]
            MemoryLibOS::Catmem(libos) => libos.sgafree(sga),
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
