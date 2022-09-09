// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod qdesc;
mod qresult;
mod qtoken;
mod qtype;

//==============================================================================
// Imports
//==============================================================================

use ::slab::Slab;

//==============================================================================
// Imports
//==============================================================================

pub use self::{
    qdesc::QDesc,
    qresult::QResult,
    qtoken::QToken,
    qtype::QType,
};

//==============================================================================
// Structures
//==============================================================================

/// IO Queue Table
pub struct IoQueueTable {
    table: Slab<u32>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for IO Queue Tables
impl IoQueueTable {
    /// Creates an IO queue table.
    pub fn new() -> Self {
        Self { table: Slab::new() }
    }

    /// Allocates a new entry in the target [IoQueueTable].
    pub fn alloc(&mut self, qtype: u32) -> QDesc {
        let ix: usize = self.table.insert(qtype);
        QDesc::from(ix)
    }

    /// Gets the file associated with an IO queue descriptor.
    pub fn get(&self, qd: QDesc) -> Option<u32> {
        if !self.table.contains(qd.into()) {
            return None;
        }

        self.table.get(qd.into()).cloned()
    }

    /// Releases an entry in the target [IoQueueTable].
    pub fn free(&mut self, qd: QDesc) -> Option<u32> {
        if !self.table.contains(qd.into()) {
            return None;
        }

        Some(self.table.remove(qd.into()))
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

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
