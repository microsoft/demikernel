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
use ::core::{
    alloc::Layout,
    mem,
    sync::atomic::{
        self,
        AtomicUsize,
    },
};
use ::std::alloc;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A lock-free, single writer and single reader, fixed-size circular buffer.
pub struct RingBuffer<T> {
    // Indexes the first empty slot after the item in the back of the ring buffer.
    back_ptr: *mut usize,
    // Indexes the first item in the front of the ring buffer.
    front_ptr: *mut usize,
    // Underlying buffer.
    buffer: raw_array::RawArray<T>,
    // Pre-computed capacity mask for the buffer.
    mask: usize,
    /// Is the underlying memory managed by this module?
    is_managed: bool,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions.
impl<T> RingBuffer<T>
where
    T: Copy,
{
    /// Creates a ring buffer.
    #[allow(unused)]
    pub fn new(capacity: usize) -> Result<RingBuffer<T>, Fail> {
        // Check if capacity is invalid.
        if !capacity.is_power_of_two() {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot create a ring buffer that does not have a power of two capacity",
            ));
        }

        let layout: Layout = Layout::new::<usize>();

        let back_ptr: *mut usize = unsafe {
            let ptr: *mut usize = alloc::alloc(layout) as *mut usize;
            if ptr.is_null() {
                alloc::handle_alloc_error(layout);
            }
            *ptr = 0;
            ptr
        };

        let front_ptr: *mut usize = unsafe {
            let ptr: *mut usize = alloc::alloc(layout) as *mut usize;
            if ptr.is_null() {
                alloc::handle_alloc_error(layout);
            }
            *ptr = 0;
            ptr
        };

        Ok(RingBuffer {
            back_ptr,
            front_ptr,
            buffer: raw_array::RawArray::<T>::new(capacity)?,
            mask: capacity - 1,
            is_managed: true,
        })
    }

    /// Constructs a ring buffer from raw parts.
    pub fn from_raw_parts(init: bool, mut ptr: *mut u8, size: usize) -> Result<RingBuffer<T>, Fail> {
        // Check if we have a valid pointer.
        if ptr.is_null() {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer from a null pointer",
            ));
        }

        // Check if the memory region is properly aligned.
        let align_of_usize: usize = mem::align_of::<usize>();
        if ptr.align_offset(align_of_usize) != 0 {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer from a unaligned memory region",
            ));
        }

        const SIZE_OF_USIZE: usize = mem::size_of::<usize>();
        let size_of_t: usize = mem::size_of::<T>();
        let mut size_of_ring: usize = SIZE_OF_USIZE + SIZE_OF_USIZE;

        // Compute pointers and required padding.
        let front_ptr: *mut usize = ptr as *mut usize;
        unsafe { ptr = ptr.add(SIZE_OF_USIZE) };
        let back_ptr: *mut usize = ptr as *mut usize;
        unsafe { ptr = ptr.add(SIZE_OF_USIZE) };
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

        // Initialize back and front pointers only if requested.
        if init {
            unsafe {
                *back_ptr = 0;
                *front_ptr = 0;
            }
        }

        Ok(RingBuffer {
            back_ptr,
            front_ptr,
            buffer: raw_array::RawArray::<T>::from_raw_parts(buffer_ptr as *mut T, len)?,
            mask: len - 1,
            is_managed: false,
        })
    }

    /// Returns the effective capacity of the target ring buffer.
    #[allow(unused)]
    pub fn capacity(&self) -> usize {
        self.buffer.capacity() - 1
    }

    /// Peeks the target ring buffer and checks if it is full.
    #[allow(unused)]
    pub fn is_full(&self) -> bool {
        let front_cached: usize = self.get_front();
        let back_cached: usize = self.get_back();

        // Check if the ring buffer is full.
        if (back_cached + 1) & self.mask == front_cached {
            return true;
        }

        false
    }

    /// Peeks the target ring buffer and checks if it is empty.
    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        let front_cached: usize = self.get_front();
        let back_cached: usize = self.get_back();

        // Check if the ring buffer is empty.
        if back_cached == front_cached {
            return true;
        }

        false
    }

    /// Attempts to insert an item at the back of the target ring buffer.
    pub fn try_enqueue(&self, item: T) -> Result<(), T> {
        let front_cached: usize = self.get_front();
        let back_cached: usize = self.get_back();

        // Check if the ring buffer is full.
        if (back_cached + 1) & self.mask == front_cached {
            return Err(item);
        }

        // Write.
        unsafe {
            let data: &mut [T] = self.buffer.get_mut();
            data[back_cached] = item;
        }

        // Commit write.
        self.set_back((back_cached + 1) & self.mask);

        Ok(())
    }

    /// Inserts an item at the back of the target ring buffer. This function may block (spin).
    #[allow(unused)]
    pub fn enqueue(&self, item: T) {
        loop {
            if self.try_enqueue(item).is_ok() {
                break;
            }
        }
    }

    /// Attempts to remove the item from the front of the target ring buffer.
    pub fn try_dequeue(&self) -> Option<T> {
        let front_cached: usize = self.get_front();
        let back_cached: usize = self.get_back();

        // Check if the ring buffer is empty.
        if back_cached == front_cached {
            return None;
        }

        // Read.
        let item: T = unsafe {
            let data: &[T] = self.buffer.get();
            data[front_cached]
        };

        // Commit read.
        self.set_front((front_cached + 1) & self.mask);

        Some(item)
    }

    /// Removes the item from the front of the target ring buffer. This function may block (spin).
    #[allow(unused)]
    pub fn dequeue(&self) -> T {
        loop {
            if let Some(item) = self.try_dequeue() {
                break item;
            }
        }
    }

    /// Atomically gets the `front` index.
    fn get_front(&self) -> usize {
        let front: &mut AtomicUsize = AtomicUsize::from_mut(unsafe { &mut *self.front_ptr });
        let front_cached: usize = front.load(atomic::Ordering::Relaxed);
        front_cached
    }

    /// Atomically sets the `front` index.
    fn set_front(&self, val: usize) {
        let front: &AtomicUsize = AtomicUsize::from_mut(unsafe { &mut *self.front_ptr });
        front.store(val, atomic::Ordering::Relaxed);
    }

    /// Atomically gets the `back` index.
    fn get_back(&self) -> usize {
        let back: &mut AtomicUsize = AtomicUsize::from_mut(unsafe { &mut *self.back_ptr });
        let back_cached: usize = back.load(atomic::Ordering::Relaxed);
        back_cached
    }

    /// Atomically sets the `back` index.
    fn set_back(&self, val: usize) {
        let back: &AtomicUsize = AtomicUsize::from_mut(unsafe { &mut *self.back_ptr });
        back.store(val, atomic::Ordering::Relaxed);
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Send trait implementation.
unsafe impl<T> Send for RingBuffer<T> {}

/// Sync trait implementation.
unsafe impl<T> Sync for RingBuffer<T> {}

/// Drop trait implementation.
impl<T> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        // Check if underlying memory was allocated by this module.
        if self.is_managed {
            // Release underlying memory.
            let layout: Layout = Layout::new::<usize>();
            unsafe {
                alloc::dealloc(self.back_ptr as *mut u8, layout);
                alloc::dealloc(self.front_ptr as *mut u8, layout);
            }
            self.is_managed = false;
        }
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use super::RingBuffer;
    use core::mem;
    use std::thread;

    /// Capacity for ring buffer.
    const RING_BUFFER_CAPACITY: usize = 4096;

    /// Creates a ring buffer with a valid capacity.
    fn do_new() -> RingBuffer<u32> {
        let ring: RingBuffer<u32> = match RingBuffer::<u32>::new(RING_BUFFER_CAPACITY) {
            Ok(ring) => ring,
            Err(_) => panic!("creating a ring buffer with valid capcity should be possible"),
        };

        // Check if buffer has expected effective capacity.
        assert!(ring.capacity() == RING_BUFFER_CAPACITY - 1);

        // Check if buffer state is consistent.
        assert!(ring.is_empty() == true);
        assert!(ring.is_full() == false);

        ring
    }

    /// Constructs a ring buffer from raw parts.
    fn do_from_raw(ptr: *mut u8, size: usize) -> RingBuffer<u32> {
        let ring: RingBuffer<u32> = match RingBuffer::<u32>::from_raw_parts(true, ptr, size) {
            Ok(ring) => ring,
            Err(e) => panic!("creating a ring buffer with valid capcity should be possible {:?}", e),
        };

        // Check if buffer has expected effective capacity.
        assert!(ring.capacity() == RING_BUFFER_CAPACITY - 1);

        // Check if buffer state is consistent.
        assert!(ring.is_empty() == true);
        assert!(ring.is_full() == false);

        ring
    }

    /// Sequentially enqueues and dequeues elements to/from a ring buffer.
    fn do_enqueue_dequeue(ring: &mut RingBuffer<u32>) {
        // Insert items in the ring buffer.
        for i in 0..ring.capacity() {
            ring.enqueue((i & 255) as u32);
        }

        // Check if buffer state is consistent.
        assert!(ring.is_empty() == false);
        assert!(ring.is_full() == true);

        // Remove items from the ring buffer.
        for i in 0..ring.capacity() {
            let item: u32 = ring.dequeue();
            assert!(item == (i & 255) as u32);
        }

        // Check if buffer state is consistent.
        assert!(ring.is_empty() == true);
        assert!(ring.is_full() == false);
    }

    /// Tests if we succeed to create a ring buffer with a valid capacity.
    #[test]
    fn new() {
        do_new();
    }

    /// Tests if we succeed to construct a ring buffer from raw parts.
    #[test]
    fn from_raw_parts() {
        const LENGTH: usize = RING_BUFFER_CAPACITY + 2 * mem::size_of::<usize>();
        const SIZE: usize = LENGTH * mem::size_of::<u32>();
        let mut array: [u32; LENGTH] = [0; LENGTH];
        do_from_raw(array.as_mut_ptr() as *mut u8, SIZE);
    }

    /// Tets if we succeed to sequentially enqueue and dequeue elements to/from a ring buffer.
    #[test]
    fn enqueue_dequeue_sequential() {
        let mut ring: RingBuffer<u32> = do_new();

        do_enqueue_dequeue(&mut ring)
    }

    /// Tets if we succeed to sequentially enqueue and dequeue elements to/from a constructed ring buffer.
    #[test]
    fn enqueue_dequeue_sequential_raw() {
        const LENGTH: usize = RING_BUFFER_CAPACITY + 2 * mem::size_of::<usize>();
        const SIZE: usize = LENGTH * mem::size_of::<u32>();
        let mut array: [u32; LENGTH] = [0; LENGTH];
        let mut ring: RingBuffer<u32> = do_from_raw(array.as_mut_ptr() as *mut u8, SIZE);

        do_enqueue_dequeue(&mut ring)
    }

    /// Tests if we fail to create ring buffer with an invalid capacity.
    #[test]
    fn bad_new() {
        match RingBuffer::<u8>::new(RING_BUFFER_CAPACITY - 1) {
            Ok(_) => panic!("creating a ring buffer with invalid capacity should fail"),
            Err(_) => {},
        };
    }

    /// Tests if we succeed to access a ring buffer concurrently.
    #[test]
    fn enqueue_dequeue_concurrent() {
        let ring: RingBuffer<u32> = do_new();

        thread::scope(|s| {
            let writer: thread::ScopedJoinHandle<()> = s.spawn(|| {
                for i in 0..ring.capacity() {
                    ring.enqueue((i & 255) as u32);
                }
            });
            let reader: thread::ScopedJoinHandle<()> = s.spawn(|| {
                for i in 0..ring.capacity() {
                    let item: u32 = ring.dequeue();
                    assert!(item == (i & 255) as u32);
                }
            });

            writer.join().unwrap();
            reader.join().unwrap();
        });
    }
}
