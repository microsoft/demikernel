// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod operation_result;
mod qdesc;
mod qtoken;
mod qtype;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::id_map::Id32Map,
    expect_some,
    runtime::{fail::Fail, scheduler::TaskWithResult},
};
use ::slab::{Iter, Slab};
use ::std::{any::Any, net::SocketAddrV4};

//======================================================================================================================
// Exports
//======================================================================================================================

pub use self::{operation_result::OperationResult, qdesc::QDesc, qtoken::QToken, qtype::QType};

// Task for running I/O operations
pub type OperationTask = TaskWithResult<(QDesc, OperationResult)>;
/// Background coroutines never return so they do not need a [ResultType].
pub type BackgroundTask = TaskWithResult<()>;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Copy)]
struct InternalId(usize);

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
    qd_to_offset: Id32Map<QDesc, InternalId>,
    table: Slab<Box<dyn IoQueue>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for I/O queue descriptors tables.
impl IoQueueTable {
    /// Allocates a new entry in the target I/O queue descriptors table.
    pub fn alloc<T: IoQueue>(&mut self, queue: T) -> QDesc {
        let index: usize = self.table.insert(Box::new(queue));
        let qd: QDesc = expect_some!(
            self.qd_to_offset.insert_with_new_id(InternalId(index)),
            "should be able to allocate an id"
        );
        trace!("inserting queue, index {:?} qd {:?}", index, qd);
        qd
    }

    /// Gets the type of the queue.
    pub fn get_type(&self, qd: &QDesc) -> Result<QType, Fail> {
        Ok(self.get_queue_ref(qd)?.get_qtype())
    }

    /// Gets/borrows a reference to the queue metadata associated with an I/O queue descriptor.
    pub fn get<'a, T: IoQueue>(&'a self, qd: &QDesc) -> Result<&'a T, Fail> {
        Ok(downcast_queue_ptr::<T>(self.get_queue_ref(qd)?)?)
    }

    /// Gets/borrows a mutable reference to the queue metadata associated with an I/O queue descriptor
    pub fn get_mut<'a, T: IoQueue>(&'a mut self, qd: &QDesc) -> Result<&'a mut T, Fail> {
        Ok(downcast_mut_ptr::<T>(self.get_mut_queue_ref(qd)?)?)
    }

    /// Releases the entry associated with an I/O queue descriptor.
    pub fn free<T: IoQueue>(&mut self, qd: &QDesc) -> Result<T, Fail> {
        let internal_id: InternalId = match self.qd_to_offset.remove(qd) {
            Some(id) => id,
            None => {
                let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
                error!("free(): {}", &cause);
                return Err(Fail::new(libc::EBADF, &cause));
            },
        };
        Ok(downcast_queue::<T>(self.table.remove(internal_id.into()))?)
    }

    /// Gets an iterator over all registered queues.
    pub fn get_values(&self) -> Iter<'_, Box<dyn IoQueue>> {
        self.table.iter()
    }

    pub fn drain(&mut self) -> slab::Drain<'_, Box<dyn IoQueue>> {
        self.table.drain()
    }

    /// Gets the index in the I/O queue descriptors table to which a given I/O queue descriptor refers to.
    fn get_queue_ref(&self, qd: &QDesc) -> Result<&Box<dyn IoQueue>, Fail> {
        if let Some(internal_id) = self.qd_to_offset.get(qd) {
            if let Some(queue) = self.table.get(internal_id.into()) {
                return Ok(queue);
            }
        }

        let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
        error!("get(): {}", &cause);
        Err(Fail::new(libc::EBADF, &cause))
    }

    fn get_mut_queue_ref(&mut self, qd: &QDesc) -> Result<&mut Box<dyn IoQueue>, Fail> {
        if let Some(internal_id) = self.qd_to_offset.get(qd) {
            if let Some(queue) = self.table.get_mut(internal_id.into()) {
                return Ok(queue);
            }
        }

        let cause: String = format!("invalid queue descriptor (qd={:?})", qd);
        error!("get(): {}", &cause);
        Err(Fail::new(libc::EBADF, &cause))
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
            qd_to_offset: Id32Map::<QDesc, InternalId>::default(),
            table: Slab::<Box<dyn IoQueue>>::new(),
        }
    }
}

impl From<InternalId> for u64 {
    fn from(val: InternalId) -> Self {
        val.0 as u64
    }
}

impl From<u32> for InternalId {
    fn from(val: u32) -> Self {
        InternalId(val as usize)
    }
}

impl From<InternalId> for u32 {
    fn from(val: InternalId) -> Self {
        TryInto::<u32>::try_into(val.0).unwrap()
    }
}

impl From<InternalId> for usize {
    /// Converts a [InternalId] to a [usize].
    fn from(val: InternalId) -> Self {
        val.0
    }
}

impl From<usize> for InternalId {
    /// Converts a [usize] to a [InternalId].
    fn from(val: usize) -> Self {
        InternalId(val)
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod tests {
    use crate::{
        expect_ok,
        runtime::{IoQueue, IoQueueTable},
        QDesc, QType,
    };
    use ::std::any::Any;
    use ::test::{black_box, Bencher};
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
    fn alloc_free_bench(b: &mut Bencher) {
        let mut ioqueue_table: IoQueueTable = IoQueueTable::default();

        b.iter(|| {
            let qd: QDesc = ioqueue_table.alloc::<TestQueue>(TestQueue {});
            black_box(qd);
            let queue: TestQueue = expect_ok!(ioqueue_table.free::<TestQueue>(&qd), "must be TestQueue");
            black_box(queue);
        });
    }
}
