// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod qdesc;
mod qresult;
mod qtoken;
mod qtype;

//======================================================================================================================
// Imports
//======================================================================================================================

use ::slab::Slab;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    qdesc::QDesc,
    qresult::QResult,
    qtoken::QToken,
    qtype::QType,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// I/O queue descriptors table.
pub struct IoQueueTable {
    // TODO: Store a QType in the slab.
    table: Slab<u32>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for I/O queue descriptors tables.
impl IoQueueTable {
    /// Offset for I/O queue descriptors.
    ///
    /// When Demikernel is interposing system calls of the underlying OS
    /// this offset enables us to distinguish I/O queue descriptors from
    /// file descriptors.
    const BASE_QD: usize = 1_000_000;

    /// Creates an I/O queue descriptors table.
    pub fn new() -> Self {
        Self { table: Slab::new() }
    }

    /// Allocates a new entry in the target I/O queue descriptors table.
    pub fn alloc(&mut self, qtype: u32) -> QDesc {
        let idx: usize = self.table.insert(qtype);
        QDesc::from(idx + Self::BASE_QD)
    }

    /// Gets the entry associated with an I/O queue descriptor.
    pub fn get(&self, qd: QDesc) -> Option<u32> {
        let idx: usize = self.get_index(qd)? as usize;
        self.table.get(idx).cloned()
    }

    /// Releases the entry associated with an I/O queue descriptor.
    pub fn free(&mut self, qd: QDesc) -> Option<u32> {
        let idx: usize = self.get_index(qd)?;
        Some(self.table.remove(idx))
    }

    /// Gets the index in the I/O queue descriptors table to which a given I/O queue descriptor refers to.
    fn get_index(&self, qd: QDesc) -> Option<usize> {
        if Into::<usize>::into(qd) < Self::BASE_QD {
            None
        } else {
            let rawqd: usize = Into::<usize>::into(qd) - Self::BASE_QD;
            if !self.table.contains(rawqd) {
                return None;
            }
            Some(rawqd)
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use super::IoQueueTable;
    use crate::{
        QDesc,
        QType,
    };
    use ::test::{
        black_box,
        Bencher,
    };

    #[bench]
    fn bench_alloc_free(b: &mut Bencher) {
        let mut ioqueue_table: IoQueueTable = IoQueueTable::new();

        b.iter(|| {
            let qd: QDesc = ioqueue_table.alloc(QType::TcpSocket.into());
            black_box(qd);
            let qtype: Option<u32> = ioqueue_table.free(qd);
            black_box(qtype);
        });
    }
}
