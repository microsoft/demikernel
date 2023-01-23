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
    const BASE_QD: u32 = 1_000_000;

    /// Creates an I/O queue descriptors table.
    pub fn new() -> Self {
        Self { table: Slab::new() }
    }

    /// Allocates a new entry in the target I/O queue descriptors table.
    pub fn alloc(&mut self, qtype: u32) -> QDesc {
        let index: usize = self.table.insert(qtype);

        // Ensure that the allocation would yield to a safe conversion between usize to u32.
        // Note: This imposes a limit on the number of open queue descriptors in u32::MAX.
        assert!(
            (index as u32) + Self::BASE_QD <= u32::MAX,
            "I/O descriptors table overflow"
        );

        QDesc::from((index as u32) + Self::BASE_QD)
    }

    /// Gets the entry associated with an I/O queue descriptor.
    pub fn get(&self, qd: QDesc) -> Option<u32> {
        let index: u32 = self.get_index(qd)?;
        self.table.get(index as usize).cloned()
    }

    /// Releases the entry associated with an I/O queue descriptor.
    pub fn free(&mut self, qd: QDesc) -> Option<u32> {
        let index: u32 = self.get_index(qd)?;
        Some(self.table.remove(index as usize))
    }

    /// Gets the index in the I/O queue descriptors table to which a given I/O queue descriptor refers to.
    fn get_index(&self, qd: QDesc) -> Option<u32> {
        if Into::<u32>::into(qd) < Self::BASE_QD {
            None
        } else {
            let rawqd: u32 = Into::<u32>::into(qd) - Self::BASE_QD;
            if !self.table.contains(rawqd as usize) {
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
