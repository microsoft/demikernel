// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

// TODO: Remove allowances on this module.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::raw_array,
    runtime::fail::Fail,
};
use ::std::alloc;
use std::{
    alloc::Layout,
    mem,
    ops::{
        Add,
        BitAnd,
        Range,
        Sub,
    },
    sync::atomic::{
        self,
        AtomicU16,
        AtomicU32,
        AtomicU64,
        AtomicU8,
        AtomicUsize,
    },
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A lock-free, single writer and single reader, fixed-size circular buffer.
pub struct RingBuffer<T, S = usize> {
    // Indexes the first empty slot after the item in the back of the ring buffer. The pointed-to value must only
    // increase monotonically.
    back_ptr: *mut S,
    // Indexes the first item in the front of the ring buffer. The pointed-to value must only increase monotonically.
    front_ptr: *mut S,
    // Underlying buffer.
    buffer: raw_array::RawArray<T>,
    // Pre-computed capacity mask for the buffer.
    mask: S,
    /// Is the underlying memory managed by this module?
    is_managed: bool,
}

/// Macro to share the common body for RingProducer and RingConsumer.
macro_rules! ring_state_struct {
    ($name:ident) => {
        /// A struct maintaining state for production or consumption of values to or from the RingBuffer.
        pub struct $name<'a, T, S = usize> {
            /// A reference to the underlying ring buffer.
            pub ring: &'a RingBuffer<T, S>,
            /// The cached value from the previous call to ring.get_back() or ring.set_back().
            pub cached_back: S,
            /// The cached value from the previous call to ring.get_front() or ring.set_front(). For producer rings,
            /// this is advanced by ring.capacity() such that cached_front is always at least as large as cached_back.
            pub cached_front: S,
        }
    };
}

ring_state_struct!(RingProducer);
ring_state_struct!(RingConsumer);

//======================================================================================================================
// Traits
//======================================================================================================================

/// An integer which holds a size value no wider than `usize`.
pub trait IntSize:
    Ord
    + Add<Self, Output = Self>
    + Sub<Self, Output = Self>
    + BitAnd<Self, Output = Self>
    + Copy
    + Default
    + TryFrom<usize>
    + From<u8>
{
    /// Atomically load with acquire semantics.
    fn atomic_load_acquire(ptr: &mut Self) -> Self;

    /// Atomically store with release semantics.
    fn atomic_store_release(ptr: &mut Self, val: Self);

    /// Casts to this integers type, as by `val as Self`. This method may truncate `val` if it does not fit in this
    /// type. When this is a concern, us `TryFrom<usize>` instead.
    fn from_usize(val: usize) -> Self;

    /// Casts to usize, as by `self as usize`. This method must be idempotent such that the following is always true:
    /// `Self::from_usize(self.to_usize()) == self`.
    fn to_usize(self) -> usize;
}

pub trait Ring: Sized {
    fn from_raw_parts(init: bool, ptr: *mut u8, size: usize) -> Result<Self, Fail>;
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions.
impl<T, S> RingBuffer<T, S>
where
    T: Copy + Sized,
    S: IntSize + Sized,
{
    /// Creates a ring buffer.
    #[allow(unused)]
    fn new<'a>(capacity: usize) -> Result<RingBuffer<T, S>, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::new");
        assert!(mem::size_of::<S>() > 0);

        // Check if capacity is invalid.
        if !capacity.is_power_of_two() {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot create a ring buffer that does not have a power of two capacity",
            ));
        }

        let s_capacity: S = match S::try_from(capacity) {
            Ok(s) => s,
            Err(_) => {
                return Err(Fail::new(
                    libc::EINVAL,
                    "capacity is too large for the configured size type",
                ))
            },
        };

        let layout: Layout = Layout::new::<S>();
        let do_alloc_s = || -> *mut S {
            // Safety: layout will have non-zero size, as validated above.
            let ptr: *mut S = unsafe { alloc::alloc(layout) } as *mut S;
            if ptr.is_null() {
                alloc::handle_alloc_error(layout);
            }
            // Safety: null-ness is checked above. `alloc` guarantees alignment and dereference-ability. Data is not
            // aliased at this point.
            unsafe { *ptr = S::default() };
            ptr
        };

        Ok(RingBuffer {
            front_ptr: do_alloc_s(),
            back_ptr: do_alloc_s(),
            buffer: raw_array::RawArray::<T>::new(capacity)?,
            mask: s_capacity - S::from(1u8),
            is_managed: true,
        })
    }

    /// Returns the effective capacity of the target ring buffer.
    #[allow(unused)]
    pub fn capacity(&self) -> S {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::capacity");
        // Safety: capacity fits in S, as validated during construction.
        S::from_usize(self.buffer.capacity() - 1)
    }

    /// Peeks the target ring buffer and checks if it is full.
    #[allow(unused)]
    pub fn is_full(&self) -> bool {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::is_null");
        let front_cached: S = self.get_front();
        let back_cached: S = self.get_back();

        // Check if the ring buffer is full.
        if (back_cached + S::from(1u8)) & self.mask == front_cached & self.mask {
            return true;
        }

        false
    }

    /// Peeks the target ring buffer and checks if it is empty.
    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::is_empty");
        let front_cached: S = self.get_front();
        let back_cached: S = self.get_back();

        // Check if the ring buffer is empty.
        if back_cached == front_cached {
            return true;
        }

        false
    }

    /// Open the ring buffer for production and consumption. This method returns a pair of objects which allow the
    /// circular buffer to be interacted with like a single-producer-single-consumer pipe.
    #[allow(unused)]
    pub fn open<'a>(&'a mut self) -> (RingProducer<'a, T, S>, RingConsumer<'a, T, S>) {
        (RingProducer::new(self), RingConsumer::new(self))
    }

    /// Atomically load and acquire the `front` index.
    fn get_front(&self) -> S {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::get_front");
        S::atomic_load_acquire(unsafe { &mut *self.front_ptr })
    }

    /// Atomically store and release the `front` index.
    #[allow(unused)]
    fn set_front(&self, val: S) {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::set_front");
        S::atomic_store_release(unsafe { &mut *self.front_ptr }, val);
    }

    /// Atomically load and acquire the `back` index.
    fn get_back(&self) -> S {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::get_back");
        S::atomic_load_acquire(unsafe { &mut *self.back_ptr })
    }

    /// Atomically store and release the `back` index.
    #[allow(unused)]
    fn set_back(&self, val: S) {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::set_back");
        S::atomic_store_release(unsafe { &mut *self.back_ptr }, val);
    }

    /// Create a slice from the underlying ring buffer according to the bounds in `range`. Does not perform any bounds
    /// checking.
    #[allow(unused)]
    unsafe fn get_unchecked_slice<'a>(&'a self, range: Range<usize>) -> &'a [T] {
        // Safety: The underlying array must be made valid/available by the RingBuffer constructor.
        let arr: &[T] = unsafe { self.buffer.get() };
        // Safety: bounds must be validated by the caller
        unsafe { &arr.get_unchecked(range) }
    }

    /// Create a mutable slice from the underlying ring buffer according to the bounds in `range`. Does not perform
    /// any bounds checking.
    #[allow(unused)]
    unsafe fn get_unchecked_mut_slice<'a>(&'a self, range: Range<usize>) -> &'a mut [T] {
        // Safety: The underlying array must be made valid/available by the RingBuffer constructor.
        let arr: &mut [T] = unsafe { self.buffer.get_mut() };
        // Safety: bounds must be validated by the caller
        unsafe { arr.get_unchecked_mut(range) }
    }
}

impl<'a, T, S> RingProducer<'a, T, S>
where
    T: Copy,
    S: IntSize,
{
    /// Creates a ring producer.
    #[allow(unused)]
    fn new(ring: &'a RingBuffer<T, S>) -> RingProducer<'a, T, S> {
        let mut ret: Self = Self {
            ring,
            cached_front: S::default(),
            cached_back: ring.get_back(),
        };
        ret.sync();
        ret
    }

    fn sync(&mut self) {
        self.cached_front = self.ring.get_front() + self.ring.capacity();
    }

    /// Get the number of free elements in the ring based on the cached state from the previous ring synchronization.
    #[allow(unused)]
    pub fn get_ready_count(&self) -> S {
        self.cached_front - self.cached_back
    }

    /// Synchronize with the underlying ring buffer and get the number of free elements in the ring. This is similar to
    /// get_ready_count, except that it first synchronizes state with the underlying RingBuffer.
    #[allow(unused)]
    pub fn sync_get_ready_count(&mut self) -> S {
        self.sync();
        self.get_ready_count()
    }

    /// Validate that a reservation of `count` elements is valid.
    #[allow(unused)]
    fn validate_enqueue(&mut self, count: S) -> Result<(), Fail> {
        // NB cached_front is advanced by buf.capacity() so we only add it when we reload the cached value
        if self.get_ready_count() >= count || self.sync_get_ready_count() >= count {
            Ok(())
        } else {
            Err(Fail::new(libc::ENOBUFS, "not enough capacity for enqueue"))
        }
    }

    /// Start an enqueue operation, consuming self and returning a new RingEnqueue. If there are not enough free slots
    /// in the ring buffer to enqueue `count` `T`'s, the operation will return an error.
    #[allow(unused)]
    pub fn try_enqueue(&mut self, val: T) -> Result<(), Fail> {
        self.validate_enqueue(S::from(1u8))?;

        let mask: S = self.ring.mask;
        let start: usize = S::to_usize(self.cached_back & mask);
        let slice: &mut [T] = unsafe { self.ring.get_unchecked_mut_slice(start..(start + 1)) };
        slice[0] = val;

        self.cached_back = self.cached_back + S::from(1u8);
        self.ring.set_back(self.cached_back);

        Ok(())
    }
}

impl<'a, T, S> RingConsumer<'a, T, S>
where
    T: Copy,
    S: IntSize,
{
    /// Creates a ring producer.
    #[allow(unused)]
    fn new(buf: &'a RingBuffer<T, S>) -> RingConsumer<'a, T, S> {
        let mut ret = Self {
            ring: buf,
            cached_front: buf.get_front(),
            cached_back: S::default(),
        };
        ret.sync();
        ret
    }

    fn sync(&mut self) {
        // NB capacity fits in S, as validated during ring construction, so no truncation takes place here.
        self.cached_back = self.ring.get_back();
    }

    /// Get the number of free elements in the ring based on the cached state from the previous ring synchronization.
    #[allow(unused)]
    pub fn get_ready_count(&self) -> S {
        self.cached_back - self.cached_front
    }

    /// Synchronize with the underlying ring buffer and get the number of free elements in the ring. This is similar to
    /// get_ready_count, except that it first synchronizes state with the underlying RingBuffer.
    #[allow(unused)]
    pub fn sync_get_ready_count(&mut self) -> S {
        self.sync();
        self.get_ready_count()
    }

    #[allow(unused)]
    fn validate_dequeue(&mut self, count: S) -> Result<(), Fail> {
        if self.get_ready_count() >= count || self.sync_get_ready_count() >= count {
            Ok(())
        } else {
            Err(Fail::new(libc::ENOBUFS, "not enough capacity for dequeue"))
        }
    }

    /// Dequeue a value from the ring. If such a value can be dequeued, calls f with the value; otherwise, returns
    #[allow(unused)]
    pub fn try_dequeue(&mut self) -> Result<T, Fail> {
        self.validate_dequeue(S::from(1u8))?;

        let mask: S = self.ring.mask;
        let start: usize = S::to_usize(self.cached_front & mask);
        let slice: &[T] = unsafe { self.ring.get_unchecked_slice(start..(start + 1)) };

        self.cached_front = self.cached_front + S::from(1u8);
        self.ring.set_front(self.cached_front);

        Ok(slice[0])
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

macro_rules! int_size_trait_impl {
    ($int_type:ident, $atomic_type:ident) => {
        impl IntSize for $int_type {
            fn atomic_load_acquire(src: &mut $int_type) -> $int_type {
                // Safety: the single mutable reference guarantees unique ownership. Memory layouts are identical for
                // support atomic types.
                let a: &$atomic_type = unsafe { &*(src as *mut $int_type).cast() };
                a.load(atomic::Ordering::Acquire)
            }

            fn atomic_store_release(dest: &mut $int_type, val: $int_type) {
                // Safety: the single mutable reference guarantees unique ownership. Memory layouts are identical for
                // support atomic types.
                let a: &$atomic_type = unsafe { &*(dest as *mut $int_type).cast() };
                a.store(val, atomic::Ordering::Release);
            }

            fn from_usize(val: usize) -> $int_type {
                val as $int_type
            }

            fn to_usize(self) -> usize {
                self as usize
            }
        }
    };
}

// Implement IntSize for all integer types which are no larger than usize. This really is a workaround for the lack of
// interconvertibility among usize and integral types.
int_size_trait_impl!(u8, AtomicU8);
int_size_trait_impl!(u16, AtomicU16);
#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
int_size_trait_impl!(u32, AtomicU32);
#[cfg(target_pointer_width = "64")]
int_size_trait_impl!(u64, AtomicU64);
int_size_trait_impl!(usize, AtomicUsize);

/// Send trait implementation.
unsafe impl<T, S> Send for RingBuffer<T, S> {}
unsafe impl<'a, T, S> Send for RingConsumer<'a, T, S> {}
unsafe impl<'a, T, S> Send for RingProducer<'a, T, S> {}

/// Drop trait implementation.
impl<T, S> Drop for RingBuffer<T, S> {
    fn drop(&mut self) {
        // Check if underlying memory was allocated by this module.
        if self.is_managed {
            // Release underlying memory.
            let layout: Layout = Layout::new::<S>();
            unsafe {
                alloc::dealloc(self.back_ptr as *mut u8, layout);
                alloc::dealloc(self.front_ptr as *mut u8, layout);
            }
            self.is_managed = false;
        }
    }
}

/// Ring trait implementation.
impl<T, S> Ring for RingBuffer<T, S>
where
    T: Copy,
    S: IntSize,
{
    /// Constructs a ring buffer from raw parts.
    fn from_raw_parts(init: bool, mut ptr: *mut u8, size: usize) -> Result<RingBuffer<T, S>, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::ring::from_raw_parts");
        // Check if we have a valid pointer.
        if ptr.is_null() {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer from a null pointer",
            ));
        }

        // Check if the memory region is properly aligned.
        let align_of_size: usize = mem::align_of::<S>();
        if ptr.align_offset(align_of_size) != 0 {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer from a unaligned memory region",
            ));
        }

        let size_of_s: usize = mem::size_of::<S>();
        let size_of_t: usize = mem::size_of::<T>();
        let mut size_of_ring: usize = size_of_s + size_of_s;

        // Compute pointers and required padding.
        let front_ptr: *mut S = ptr as *mut S;
        unsafe { ptr = ptr.add(size_of_s) };
        let back_ptr: *mut S = ptr as *mut S;
        unsafe { ptr = ptr.add(size_of_s) };
        let buffer_ptr: *mut u8 = {
            let padding: usize = ptr.align_offset(size_of_t);
            size_of_ring += padding;
            unsafe { ptr.add(padding) }
        };

        // Check if memory region is big enough.
        if size < (size_of_ring + size_of_t) {
            return Err(Fail::new(
                libc::EINVAL,
                "memory region is too small to fit in a ring buffer",
            ));
        }

        // Compute length of buffer.
        // It should be the highest power of two that fits in.
        let len: usize = {
            let maxlen: usize = (size - size_of_ring) / size_of_t;
            1 << maxlen.ilog2()
        };

        let len_s: S = match S::try_from(len) {
            Ok(s) => s,
            Err(_) => {
                return Err(Fail::new(
                    libc::EINVAL,
                    "memory region contains too many elements to be indexed by the specified type",
                ))
            },
        };

        // Initialize back and front pointers only if requested.
        if init {
            unsafe {
                *back_ptr = S::default();
                *front_ptr = S::default();
            }
        }

        Ok(RingBuffer {
            back_ptr,
            front_ptr,
            buffer: raw_array::RawArray::<T>::from_raw_parts(buffer_ptr as *mut T, len)?,
            mask: len_s - S::from(1u8),
            is_managed: false,
        })
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use super::{
        Ring,
        RingBuffer,
        RingConsumer,
        RingProducer,
    };
    use ::anyhow::Result;
    use ::core::mem;
    use ::std::thread;
    use std::sync::{
        Arc,
        Barrier,
    };

    /// Capacity for ring buffer.
    const RING_BUFFER_CAPACITY: usize = 4096;

    /// Creates a ring buffer with a valid capacity.
    fn do_new() -> Result<RingBuffer<u32>> {
        let ring: RingBuffer<u32> = match RingBuffer::<u32>::new(RING_BUFFER_CAPACITY) {
            Ok(ring) => ring,
            Err(_) => anyhow::bail!("creating a ring buffer with valid capcity should be possible"),
        };

        // Check if buffer has expected effective capacity.
        crate::ensure_eq!(ring.capacity(), RING_BUFFER_CAPACITY - 1);

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), true);
        crate::ensure_eq!(ring.is_full(), false);

        Ok(ring)
    }

    /// Constructs a ring buffer from raw parts.
    fn do_from_raw(ptr: *mut u8, size: usize) -> Result<RingBuffer<u32>> {
        let ring: RingBuffer<u32> = match RingBuffer::<u32>::from_raw_parts(true, ptr, size) {
            Ok(ring) => ring,
            Err(e) => anyhow::bail!("creating a ring buffer with valid capcity should be possible {:?}", e),
        };

        // Check if buffer has expected effective capacity.
        crate::ensure_eq!(ring.capacity(), RING_BUFFER_CAPACITY - 1);

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), true);
        crate::ensure_eq!(ring.is_full(), false);

        Ok(ring)
    }

    /// Sequentially enqueues and dequeues elements to/from a ring buffer.
    fn do_enqueue_dequeue(ring: &mut RingBuffer<u32>) -> Result<()> {
        let capacity: usize = ring.capacity();

        {
            let (mut prod, _): (RingProducer<u32>, RingConsumer<u32>) = ring.open();
            crate::ensure_eq!(prod.get_ready_count(), capacity);

            // Insert items in the ring buffer.
            for i in 0..prod.get_ready_count() {
                prod.try_enqueue((i & 255) as u32)?;
            }
        }

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), false);
        crate::ensure_eq!(ring.is_full(), true);

        {
            let (_, mut cons): (RingProducer<u32>, RingConsumer<u32>) = ring.open();
            crate::ensure_eq!(cons.get_ready_count(), capacity);

            // Remove items from the ring buffer.
            for i in 0..cons.get_ready_count() {
                let item = cons.try_dequeue()?;
                crate::ensure_eq!(item, (i & 255) as u32);
            }
        }

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), true);
        crate::ensure_eq!(ring.is_full(), false);

        Ok(())
    }

    /// Tests if we succeed to create a ring buffer with a valid capacity.
    #[test]
    fn new() -> Result<()> {
        do_new()?;
        Ok(())
    }

    /// Tests if we succeed to construct a ring buffer from raw parts.
    #[test]
    fn from_raw_parts() -> Result<()> {
        const LENGTH: usize = RING_BUFFER_CAPACITY + 2 * mem::size_of::<usize>();
        const SIZE: usize = LENGTH * mem::size_of::<u32>();
        let mut array: [u32; LENGTH] = [0; LENGTH];
        do_from_raw(array.as_mut_ptr() as *mut u8, SIZE)?;
        Ok(())
    }

    /// Tets if we succeed to sequentially enqueue and dequeue elements to/from a ring buffer.
    #[test]
    fn enqueue_dequeue_sequential() -> Result<()> {
        let mut ring: RingBuffer<u32> = do_new()?;

        do_enqueue_dequeue(&mut ring)
    }

    /// Tets if we succeed to sequentially enqueue and dequeue elements to/from a constructed ring buffer.
    #[test]
    fn enqueue_dequeue_sequential_raw() -> Result<()> {
        const LENGTH: usize = RING_BUFFER_CAPACITY + 2 * mem::size_of::<usize>();
        const SIZE: usize = LENGTH * mem::size_of::<u32>();
        let mut array: [u32; LENGTH] = [0; LENGTH];
        let mut ring: RingBuffer<u32> = do_from_raw(array.as_mut_ptr() as *mut u8, SIZE)?;

        do_enqueue_dequeue(&mut ring)
    }

    /// Tests if we fail to create ring buffer with an invalid capacity.
    #[test]
    fn bad_new() -> Result<()> {
        match RingBuffer::<u8>::new(RING_BUFFER_CAPACITY - 1) {
            Ok(_) => anyhow::bail!("creating a ring buffer with invalid capacity should fail"),
            Err(_) => Ok(()),
        }
    }

    /// Tests if we succeed to access a ring buffer concurrently.
    #[test]
    fn enqueue_dequeue_concurrent() -> Result<()> {
        let mut ring: RingBuffer<u32> = do_new()?;
        let mut result: Result<()> = Ok(());

        thread::scope(|s| {
            let start_barrier: Arc<Barrier> = Arc::new(Barrier::new(2));
            let capacity: usize = ring.capacity();
            let (mut prod, mut cons) = ring.open();
            let reader: thread::ScopedJoinHandle<Result<()>> = {
                let start_barrier = start_barrier.clone();
                s.spawn(move || {
                    start_barrier.wait();
                    for i in 0..capacity {
                        loop {
                            if let Ok(item) = cons.try_dequeue() {
                                crate::ensure_eq!(item, (i & 255) as u32);
                                break;
                            };
                        }
                    }
                    Ok(())
                })
            };

            // NB Ensure the reader is started before starting the writer. This increases the likelihood that the reader
            // will fail to dequeue for some iterations.
            start_barrier.wait();

            let writer: thread::ScopedJoinHandle<Result<()>> = s.spawn(move || {
                for i in 0..capacity {
                    prod.try_enqueue((i & 255) as u32)?;
                }
                Ok(())
            });

            result = writer.join().unwrap().and(reader.join().unwrap());
        });

        result
    }
}
