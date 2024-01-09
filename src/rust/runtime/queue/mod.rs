// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod operation_result;
mod qdesc;
mod qtoken;
mod qtype;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    fail::Fail,
    scheduler::TaskWithResult,
};
use ::futures::future::FusedFuture;
use ::slab::{
    Iter,
    Slab,
};
use ::std::{
    any::Any,
    net::SocketAddrV4,
};

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{
    operation_result::OperationResult,
    qdesc::QDesc,
    qtoken::QToken,
    qtype::QType,
};

// Coroutine for running an operation on an I/O Queue.
pub type Operation = dyn FusedFuture<Output = (QDesc, OperationResult)>;
// Task for running I/O operations
pub type OperationTask = TaskWithResult<(QDesc, OperationResult)>;
/// Background coroutines never return so they do not need a [ResultType].
pub type BackgroundTask = TaskWithResult<()>;

//======================================================================================================================
// Structures
//======================================================================================================================

pub trait IoQueue: Any {
    fn get_qtype(&self) -> QType;
    fn as_any_ref(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(self: Box<Self>) -> Box<dyn Any>;
}

pub trait NetworkQueue: IoQueue {
    fn local(&self) -> Option<SocketAddrV4>;
    fn remote(&self) -> Option<SocketAddrV4>;
}

/// I/O queue descriptors table.
pub struct IoQueueTable {
    table: Slab<Box<dyn IoQueue>>,
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
    ///
    /// NOTE: This is intentionally set to be half of FD_SETSIZE (1024) in Linux.
    const BASE_QD: u32 = 500;

    /// Allocates a new entry in the target I/O queue descriptors table.
    pub fn alloc<T: IoQueue>(&mut self, queue: T) -> QDesc {
        let index: usize = self.table.insert(Box::new(queue));

        // Ensure that the allocation would yield to a safe conversion between usize to u32.
        // Note: This imposes a limit on the number of open queue descriptors in u32::MAX.
        assert!(
            (index as u32) + Self::BASE_QD <= u32::MAX,
            "I/O descriptors table overflow"
        );

        QDesc::from((index as u32) + Self::BASE_QD)
    }

    /// Gets the type of the queue.
    pub fn get_type(&self, qd: &QDesc) -> Result<QType, Fail> {
        let index: u32 = match self.get_index(qd) {
            Some(index) => index,
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_type(): {}", &cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        };
        match self.table.get(index as usize) {
            Some(boxed_queue_ptr) => Ok(boxed_queue_ptr.get_qtype()),
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_type(): {}", &cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    /// Gets/borrows a reference to the queue metadata associated with an I/O queue descriptor.
    pub fn get<'a, T: IoQueue>(&'a self, qd: &QDesc) -> Result<&'a T, Fail> {
        let index: u32 = match self.get_index(qd) {
            Some(index) => index,
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get(): {}", &cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        };
        match self.table.get(index as usize) {
            Some(boxed_queue_ptr) => Ok(downcast_queue_ptr::<T>(boxed_queue_ptr)?),
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get(): {}", &cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    /// Gets/borrows a mutable reference to the queue metadata associated with an I/O queue descriptor
    pub fn get_mut<'a, T: IoQueue>(&'a mut self, qd: &QDesc) -> Result<&'a mut T, Fail> {
        let index: u32 = match self.get_index(qd) {
            Some(index) => index,
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_mut(): {}", &cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        };
        match self.table.get_mut(index as usize) {
            Some(boxed_queue_ptr) => Ok(downcast_mut_ptr::<T>(boxed_queue_ptr)?),
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("get_mut(): {}", &cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    /// Releases the entry associated with an I/O queue descriptor.
    pub fn free<T: IoQueue>(&mut self, qd: &QDesc) -> Result<T, Fail> {
        let index: u32 = match self.get_index(qd) {
            Some(index) => index,
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("free(): {}", &cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        };
        Ok(downcast_queue::<T>(self.table.remove(index as usize))?)
    }

    /// Gets an iterator over all registered queues.
    pub fn get_values(&self) -> Iter<'_, Box<dyn IoQueue>> {
        self.table.iter()
    }

    pub fn drain(&mut self) -> slab::Drain<'_, Box<dyn IoQueue>> {
        self.table.drain()
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
// Standalone functions
//======================================================================================================================

/// Downcasts a [IoQueue] reference to a concrete queue type reference `&T`.
pub fn downcast_queue_ptr<'a, T: IoQueue>(boxed_queue_ptr: &'a Box<dyn IoQueue>) -> Result<&'a T, Fail> {
    // 1. Get reference to queue inside the box.
    let queue_ptr: &dyn IoQueue = boxed_queue_ptr.as_ref();
    // 2. Cast that reference to a void pointer for downcasting.
    let void_ptr: &dyn Any = queue_ptr.as_any_ref();
    // 3. Downcast to concrete type T
    match void_ptr.downcast_ref::<T>() {
        Some(ptr) => Ok(ptr),
        None => {
            let cause: String = format!("invalid queue type");
            error!("downcast_queue_ptr(): {}", &cause);
            Err(Fail::new(libc::EINVAL, &cause))
        },
    }
}

pub fn downcast_mut_ptr<'a, T: IoQueue>(boxed_queue_ptr: &'a mut Box<dyn IoQueue>) -> Result<&'a mut T, Fail> {
    // 1. Get reference to queue inside the box.
    let queue_ptr: &mut dyn IoQueue = boxed_queue_ptr.as_mut();
    // 2. Cast that reference to a void pointer for downcasting.
    let void_ptr: &mut dyn Any = queue_ptr.as_any_mut();
    // 3. Downcast to concrete type T
    match void_ptr.downcast_mut::<T>() {
        Some(ptr) => Ok(ptr),
        None => {
            let cause: String = format!("invalid queue type");
            error!("downcast_mut_ptr(): {}", &cause);
            Err(Fail::new(libc::EINVAL, &cause))
        },
    }
}

/// Downcasts a boxed [IoQueue] to a concrete queue type `T`.
pub fn downcast_queue<T: IoQueue>(boxed_queue: Box<dyn IoQueue>) -> Result<T, Fail> {
    // 1. Downcast from boxed type to concrete type T
    match boxed_queue.as_any().downcast::<T>() {
        Ok(queue) => Ok(*queue),
        Err(_) => {
            let cause: String = format!("invalid queue type");
            error!("downcast_queue(): {}", &cause);
            Err(Fail::new(libc::EINVAL, &cause))
        },
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Default for IoQueueTable {
    fn default() -> Self {
        Self {
            table: Slab::<Box<dyn IoQueue>>::new(),
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
    use ::std::any::Any;
    use ::test::{
        black_box,
        Bencher,
    };
    pub struct TestQueue {}

    impl IoQueue for TestQueue {
        fn get_qtype(&self) -> QType {
            QType::TestQueue
        }

        fn as_any_ref(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn as_any(self: Box<Self>) -> Box<dyn Any> {
            self
        }
    }

    #[bench]
    fn bench_alloc_free(b: &mut Bencher) {
        let mut ioqueue_table: IoQueueTable = IoQueueTable::default();

        b.iter(|| {
            let qd: QDesc = ioqueue_table.alloc::<TestQueue>(TestQueue {});
            black_box(qd);
            let queue: TestQueue = ioqueue_table.free::<TestQueue>(&qd).expect("must be TestQueue");
            black_box(queue);
        });
    }
}
