// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod qdesc;
mod qresult;
mod qtoken;
mod qtype;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::scheduler::TaskWithResult;
use ::slab::{
    Iter,
    Slab,
};
use ::std::future::Future;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    qdesc::QDesc,
    qresult::{
        OperationResult,
        QResult,
    },
    qtoken::QToken,
    qtype::QType,
};

// Coroutine for running an operation on an I/O Queue.
pub type Operation = dyn Future<Output = (QDesc, OperationResult)>;
// Task for running I/O operations
pub type OperationTask = TaskWithResult<(QDesc, OperationResult)>;
/// Background coroutines never return so they do not need a [ResultType].
pub type BackgroundTask = TaskWithResult<()>;

//======================================================================================================================
// Structures
//======================================================================================================================

pub trait IoQueue {
    fn get_qtype(&self) -> QType;
}

/// I/O queue descriptors table.
pub struct IoQueueTable<T: IoQueue> {
    table: Slab<T>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for I/O queue descriptors tables.
impl<T: IoQueue> IoQueueTable<T> {
    /// Offset for I/O queue descriptors.
    ///
    /// When Demikernel is interposing system calls of the underlying OS
    /// this offset enables us to distinguish I/O queue descriptors from
    /// file descriptors.
    ///
    /// NOTE: This is intentionally set to be half of FD_SETSIZE (1024) in Linux.
    const BASE_QD: u32 = 500;

    /// Creates an I/O queue descriptors table.
    pub fn new() -> Self {
        Self {
            table: Slab::<T>::new(),
        }
    }

    /// Allocates a new entry in the target I/O queue descriptors table.
    pub fn alloc(&mut self, queue: T) -> QDesc {
        let index: usize = self.table.insert(queue);

        // Ensure that the allocation would yield to a safe conversion between usize to u32.
        // Note: This imposes a limit on the number of open queue descriptors in u32::MAX.
        assert!(
            (index as u32) + Self::BASE_QD <= u32::MAX,
            "I/O descriptors table overflow"
        );

        QDesc::from((index as u32) + Self::BASE_QD)
    }

    /// Gets/borrows a reference to the queue metadata associated with an I/O queue descriptor.
    pub fn get(&self, qd: &QDesc) -> Option<&T> {
        let index: u32 = self.get_index(qd)?;
        self.table.get(index as usize)
    }

    /// Gets/borrows a mutable reference to the queue metadata associated with an I/O queue descriptor
    pub fn get_mut(&mut self, qd: &QDesc) -> Option<&mut T> {
        let index: u32 = self.get_index(qd)?;
        self.table.get_mut(index as usize)
    }

    /// Releases the entry associated with an I/O queue descriptor.
    pub fn free(&mut self, qd: &QDesc) -> Option<T> {
        let index: u32 = self.get_index(qd)?;
        Some(self.table.remove(index as usize))
    }

    /// Gets an iterator over all registered queues.
    pub fn get_values(&self) -> Iter<'_, T> {
        self.table.iter()
    }

    /// Gets the index in the I/O queue descriptors table to which a given I/O queue descriptor refers to.
    fn get_index(&self, qd: &QDesc) -> Option<u32> {
        if Into::<u32>::into(*qd) < Self::BASE_QD {
            None
        } else {
            let rawqd: u32 = Into::<u32>::into(*qd) - Self::BASE_QD;
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
    use super::{
        IoQueue,
        IoQueueTable,
    };
    use crate::{
        QDesc,
        QType,
    };
    use ::test::{
        black_box,
        Bencher,
    };

    pub struct TestQueue {}

    impl IoQueue for TestQueue {
        fn get_qtype(&self) -> QType {
            QType::TestQueue
        }
    }

    #[bench]
    fn bench_alloc_free(b: &mut Bencher) {
        let mut ioqueue_table: IoQueueTable<TestQueue> = IoQueueTable::<TestQueue>::new();

        b.iter(|| {
            let qd: QDesc = ioqueue_table.alloc(TestQueue {});
            black_box(qd);
            let qtype: Option<TestQueue> = ioqueue_table.free(&qd);
            black_box(qtype);
        });
    }
}
