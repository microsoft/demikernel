// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::{
        raw_array,
        ring::Ring,
    },
    runtime::{
        fail::Fail,
        DemiRuntime,
    },
};
use ::core::{
    alloc::Layout,
    mem,
    sync::atomic::{
        self,
        AtomicU16,
        AtomicUsize,
    },
};
use ::std::{
    alloc,
    ptr::copy,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Size of header in bytes = 16-bit buffer length.
const HEADER_SIZE: usize = 2;

//======================================================================================================================
// Structures
//======================================================================================================================

/// |HEADER = LEN(PAYLOAD) | PAYLOAD | possibly 1 byte padding |

/// A lock-free, multi-reader, multi-writer, variable-sized messaging queue. It uses `push_offset` to indicate where
/// to write incoming messages and `pop_offset` for where to read incoming messages. Additionally each message contains
/// a [HEADER_SIZE] header that indicates the length of the data (and as a result, pointing to the next valid buffer in
/// the ring plus some alignment). This header also serves as a locking mechanism: on push, the writer atomically sets
/// the header to indicated valid data that is ready to be read; on pop, the reader atomically sets the header to 0 in
/// order to "claim"/lock the message from other concurrent readers.
///
/// For correctness, the following invariants
/// must hold:
/// 0. push_offset and pop_offset must monotonically increase, except when wrapping around, and must always be aligned
///    with [HEADER_SIZE].
/// 1. push_offset == pop_offset only if queue is empty.
/// 2. push_offset == pop_offset - [HEADER_SIZE] only if queue is full. We only utilize capacity - [HEADER_SIZE] of the
///    ring buffer so that we can distinguish between these two states.
/// 3. First [HEADER_SIZE] bytes of every buffer is a header indicating the length of valid data in the .
/// 4. If the message header is non-zero, the data in the payload must be valid.
/// 5. If the message is valid (i.e., the message header is non-zero), it must exist between the push_offset and
///    pop_offset. As a result, the pop_offset can never overtake the push_offset.
/// 6. If the length header is zero, the data in the buffer may be valid but if it is valid, it is locked and should
/// not be modified or read.
/// 7. If the 16 bytes pointed to by pop_offset are zero, then there is either another ongoing pop or there is no valid
/// data in the buffer.
pub struct ConcurrentRingBuffer {
    // Indexes the first empty byte where buffers can be enqueued.
    push_offset: *mut usize,
    // Indexes the first buffer that can be popped.
    pop_offset: *mut usize,
    // Underlying buffer.
    buffer: raw_array::RawArray<u8>,
    /// Is the underlying memory managed by this module?
    is_managed: bool,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions.
impl ConcurrentRingBuffer {
    /// Creates a ring buffer.
    #[allow(unused)]
    pub fn new(capacity: usize) -> Result<Self, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::new");
        // Check if capacity is invalid.
        if capacity == 0 {
            let cause: String = format!("invalid capacity (capacity={})", capacity);
            error!("new(): {}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        let push_offset: *mut usize = Self::alloc::<usize>()?;
        unsafe { *push_offset = 0 };

        let pop_offset: *mut usize = Self::alloc::<usize>()?;
        unsafe { *pop_offset = 0 };

        let me: Self = Self {
            pop_offset,
            push_offset,
            buffer: raw_array::RawArray::<u8>::new(capacity)?,
            is_managed: true,
        };

        // Initialize the first header to 0.
        me.write_header(0, 0);

        Ok(me)
    }

    /// Returns the effective capacity of the target ring buffer in bytes.
    #[allow(unused)]
    pub fn capacity(&self) -> usize {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::capacity");
        self.buffer.capacity()
    }

    #[allow(unused)]
    pub fn remaining_capacity(&self) -> usize {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::remaining_capacity");
        let push_offset: usize = peek(self.push_offset);
        let pop_offset: usize = peek(self.pop_offset);
        self.available_space(push_offset, pop_offset)
    }

    /// Attempts to insert a buffer of [len] bytes into the ring buffer.
    pub fn try_push(&self, buf: &[u8]) -> Result<usize, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::try_push");
        let len: usize = buf.len();
        if len == 0 {
            return Err(Fail::new(libc::EINVAL, "Buffer must be non-zero length"));
        }
        // reserve_space will allocate space for the header.
        if let Some(push_offset) = self.reserve_space(len) {
            debug_assert!(push_offset % HEADER_SIZE == 0);
            // Push first part of buffer. If longer than the capacity of the ring, wrap around.
            let first_offset: usize = push_offset + HEADER_SIZE;
            let first_len: usize = if push_offset + len + HEADER_SIZE > self.capacity() {
                self.capacity() - first_offset
            } else {
                len
            };
            let buf_ptr: *const u8 = buf.as_ptr();
            let ring_ptr: *mut u8 = unsafe { self.buffer.get_mut().as_mut_ptr() };
            // Copy the data into the ring buffer.
            unsafe {
                copy(buf_ptr, ring_ptr.add(first_offset), first_len);
            }
            // If there is remaining data in the buffer, wrap around.
            if len > first_len {
                // Copy the data into the ring buffer.
                unsafe {
                    copy(buf_ptr.add(first_len), ring_ptr, len - first_len);
                }
            }
            // Commit the write by atomically writing the header to release the buffer. The overwritten header MUST be
            // 0. The header describes just the length of the payload.
            let old: usize = self.write_header(push_offset, len);
            debug_assert_eq!(old, 0);
            trace!(
                "try_push() len={:?} push_offset={:?} pop_offset={:?}",
                len,
                peek(self.push_offset),
                peek(self.pop_offset)
            );

            Ok(len)
        } else {
            Err(Fail::new(libc::EAGAIN, "No space in the ring buffer"))
        }
    }

    /// Inserts an item at the enqueue of the target ring buffer. This function may block (spin).
    #[allow(unused)]
    pub fn push(&self, buf: &[u8]) -> Option<usize> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::push");
        loop {
            match self.try_push(buf) {
                Ok(len) => return Some(len),
                Err(Fail { errno, cause: _ }) if DemiRuntime::should_retry(errno) => continue,
                Err(e) => return None,
            }
        }
    }

    /// Attempts to remove next message from the ring buffer up to [len] bytes and copies into [buf]. This function
    /// does not block.
    pub fn try_pop(&self, buf: &mut [u8]) -> Result<usize, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::try_pop");
        let len: usize = buf.len();
        if len == 0 {
            return Err(Fail::new(libc::EINVAL, "Buffer must be non-zero length"));
        }

        // Is this safe or do we need an atomic indirect read?
        let pop_offset: usize = peek(self.pop_offset);
        // This represents the total length of the incoming message.
        let pop_len: usize = match self.write_header(pop_offset, 0) {
            0 => return Err(Fail::new(libc::EAGAIN, "No messages in the ring buffer")),
            bytes if bytes <= len => bytes,
            bytes => {
                // Buffer is not big enough so put the message back in the queue.
                // We know that the pop_offset did not move because it was not pointing at a valid message.
                let old_len: usize = self.write_header(pop_offset, bytes);
                debug_assert_eq!(old_len, 0);
                return Err(Fail::new(libc::EINVAL, "Buffer is too small to hold next message"));
            },
        };

        // Pop first part of buffer.
        let first_offset: usize = pop_offset + HEADER_SIZE;
        let first_len: usize = if pop_offset + pop_len + HEADER_SIZE > self.capacity() {
            self.capacity() - first_offset
        } else {
            len
        };
        let buf_ptr: *mut u8 = buf.as_mut_ptr();
        let ring_ptr: *const u8 = unsafe { self.buffer.get().as_ptr() };
        // Copy the data into the ring buffer.
        unsafe {
            copy(ring_ptr.add(first_offset), buf_ptr, first_len);
        }
        // If there is remaining data in the buffer, wrap around.
        if len > first_len {
            // Copy the data into the ring buffer.
            unsafe {
                copy(ring_ptr, buf_ptr.add(first_len), len - first_len);
            }
        }

        // Move to next buffer.
        self.release_space(pop_offset, pop_len);
        trace!(
            "try_push() len={:?} push_offset={:?} pop_offset={:?}",
            len,
            peek(self.push_offset),
            peek(self.pop_offset)
        );
        Ok(pop_len)
    }

    /// Removes the next message from the ring buffer up to [len] bytes and copies into [buf]. This function may block
    /// (spin).
    #[allow(unused)]
    pub fn pop(&self, buf: &mut [u8]) -> Option<usize> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::pop");
        loop {
            match self.try_pop(buf) {
                Ok(len) => return Some(len),
                Err(Fail { errno, cause: _ }) if DemiRuntime::should_retry(errno) => continue,
                Err(e) => return None,
            }
        }
    }

    /// Atomically writes a header at the indicated offset and returns the previous one.
    fn write_header(&self, offset: usize, val: usize) -> usize {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::write_header");
        assert!(offset % 2 == 0);
        let buffer_ptr: *mut u8 = unsafe { self.buffer.get_mut() }.as_mut_ptr();
        let header_ptr: *mut u16 = unsafe { buffer_ptr.add(offset) } as *mut u16;
        let header: &AtomicU16 = unsafe { &*header_ptr.cast() };
        header.swap(val as u16, atomic::Ordering::Relaxed) as usize
    }

    /// Given a [push_offset] and [pop_offset] into the ring buffer, return available space for writing data. Always
    /// leave one [HEADER_SIZE] space for distinguishing a full from empty buffer.
    fn available_space(&self, push_offset: usize, pop_offset: usize) -> usize {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::available_space");
        debug_assert!(push_offset + self.capacity() > pop_offset);
        let used_space: usize = (push_offset + self.capacity() - pop_offset) % self.capacity();
        debug_assert!(self.capacity() > used_space + HEADER_SIZE);
        self.capacity() - used_space - HEADER_SIZE
    }

    /// Reserves [len] + HEADER_SIZE bytes from the ring buffer. If successful, returns the offset of the beginning of
    /// the buffer, else returns `None`.
    fn reserve_space(&self, len: usize) -> Option<usize> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::reserve_space");
        let len_: usize = align_header(len + HEADER_SIZE);
        let push_offset: usize = peek(self.push_offset);
        let pop_offset: usize = peek(self.pop_offset);

        if len_ > self.available_space(push_offset, pop_offset) {
            return None;
        }
        let new_offset: usize = (push_offset + len_) % self.capacity();

        debug_assert_ne!(new_offset, pop_offset);
        // Queue has space after the enqueue pointer, so try to reserve space.
        match check_and_set(self.push_offset, push_offset, new_offset) {
            Ok(start) => {
                self.write_header(new_offset, 0);
                Some(start)
            },
            Err(_) => None,
        }
    }

    /// Frees [len] + HEADER_SIZE bytes from the ring buffer.
    fn release_space(&self, current_offset: usize, len: usize) {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::release_space");
        let len_: usize = align_header(len + HEADER_SIZE);
        let new_offset: usize = (current_offset + len_) % self.capacity();
        // Ensure that the old pop_offset was what we expected. Panic if it is not.
        check_and_set(self.pop_offset, current_offset, new_offset).unwrap();
    }

    #[allow(unused)]
    pub fn is_full(&self) -> bool {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::is_full");
        let push_offset = peek(self.push_offset);
        let pop_offset = peek(self.pop_offset);

        self.available_space(push_offset, pop_offset) == 0
    }

    /// Peeks the target ring buffer and checks if it is empty.
    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::is_empty");
        let push_offset = peek(self.push_offset);
        let pop_offset = peek(self.pop_offset);

        pop_offset == push_offset
    }

    /// Allocates a memory area.
    fn alloc<T>() -> Result<*mut T, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::alloc");
        let layout: Layout = Layout::new::<T>();
        let ptr: *mut T = unsafe { alloc::alloc(layout) as *mut T };
        if ptr.is_null() {
            alloc::handle_alloc_error(layout);
        }

        unsafe { *ptr = mem::zeroed() };

        Ok(ptr)
    }
}

impl Ring for ConcurrentRingBuffer {
    /// Constructs a ring buffer from raw parts. [size] indicates the size of the raw parts, while [capacity] indicates
    /// the amount of storage space in bytes that should be available. [size] must be at least large enough to hold
    /// capacity, plus the push and pop offsets and a [HEADER_SIZE] piece of padding.
    fn from_raw_parts(init: bool, ptr: *mut u8, capacity: usize) -> Result<Self, Fail> {
        #[cfg(feature = "profiler")]
        timer!("collections::concurrent_ring::from_raw_parts");
        // Check if we have a valid pointer.
        if ptr.is_null() {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer from a null pointer",
            ));
        }

        // Check if the memory region is aligned to usize.
        let align_of_usize: usize = mem::align_of::<usize>();
        if ptr.align_offset(align_of_usize) != 0 {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer from a unaligned memory region",
            ));
        }

        const SIZE_OF_USIZE: usize = mem::size_of::<usize>();
        // Check that there is sufficient space in the buffer.
        let size_of_ring: usize = 2 * SIZE_OF_USIZE;
        if capacity <= size_of_ring {
            return Err(Fail::new(
                libc::EINVAL,
                "cannot construct a ring buffer with insufficeint space",
            ));
        }

        // Compute pointers and required padding.
        let mut buffer_ptr: *mut u8 = ptr;
        let pop_offset: *mut usize = buffer_ptr as *mut usize;
        buffer_ptr = unsafe { buffer_ptr.add(SIZE_OF_USIZE) };
        let push_offset: *mut usize = buffer_ptr as *mut usize;
        buffer_ptr = unsafe { buffer_ptr.add(SIZE_OF_USIZE) };

        // Initialize enqueue and dequeue pointers only if requested.
        if init {
            unsafe {
                *push_offset = 0;
                *pop_offset = 0;
            }
        }

        let me: Self = Self {
            push_offset,
            pop_offset,
            buffer: raw_array::RawArray::<u8>::from_raw_parts(buffer_ptr, capacity - size_of_ring)?,
            is_managed: false,
        };
        // Intialize the header to 0.
        me.write_header(0, 0);
        Ok(me)
    }
}
//======================================================================================================================
// Stand-alone functions
//======================================================================================================================

/// Peeks at the value at [ptr] to check various constraints.
fn peek(ptr: *mut usize) -> usize {
    let ptr: &AtomicUsize = unsafe { &*ptr.cast() };
    ptr.load(atomic::Ordering::Relaxed)
}

/// Compares and increments the value at [ptr] only if it has not changed since the last time we read it.
fn check_and_set(ptr: *mut usize, current: usize, new: usize) -> Result<usize, usize> {
    let ptr: &AtomicUsize = unsafe { &*ptr.cast() };
    ptr.compare_exchange_weak(current, new, atomic::Ordering::Acquire, atomic::Ordering::Relaxed)
}

/// Align to [HEADER_SIZE] for the header offset.
fn align_header(offset: usize) -> usize {
    // Round up to u16 aligned.
    match offset % HEADER_SIZE {
        0 => offset,
        x => offset - x + HEADER_SIZE,
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Send trait implementation.
unsafe impl Send for ConcurrentRingBuffer {}

/// Sync trait implementation.
unsafe impl Sync for ConcurrentRingBuffer {}

/// Drop trait implementation.
impl Drop for ConcurrentRingBuffer {
    fn drop(&mut self) {
        // Check if underlying memory was allocated by this module.
        if self.is_managed {
            // Release underlying memory.
            let layout: Layout = Layout::new::<usize>();
            unsafe {
                alloc::dealloc(self.push_offset as *mut u8, layout);
                alloc::dealloc(self.pop_offset as *mut u8, layout);
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
    use super::{
        ConcurrentRingBuffer,
        Ring,
    };
    use ::anyhow::Result;
    use ::core::mem;
    use ::std::thread;
    use std::{
        ops::Range,
        time::Duration,
    };

    /// Capacity for ring buffer in bytes.
    const RING_BUFFER_CAPACITY: usize = 4096;
    const ITERATIONS: usize = 128;

    /// Creates a ring buffer with a valid capacity.
    fn do_new() -> Result<ConcurrentRingBuffer> {
        let ring: ConcurrentRingBuffer = match ConcurrentRingBuffer::new(RING_BUFFER_CAPACITY) {
            Ok(ring) => ring,
            Err(_) => anyhow::bail!("creating a ring buffer with valid capcity should be possible"),
        };

        // Check if buffer has expected effective capacity.
        crate::ensure_eq!(ring.capacity(), RING_BUFFER_CAPACITY);

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), true);
        crate::ensure_eq!(ring.is_full(), false);

        Ok(ring)
    }

    /// Constructs a ring buffer from raw parts.
    fn do_from_raw(ptr: *mut u8, size: usize) -> Result<ConcurrentRingBuffer> {
        let ring: ConcurrentRingBuffer = match ConcurrentRingBuffer::from_raw_parts(true, ptr, size) {
            Ok(ring) => ring,
            Err(e) => anyhow::bail!("creating a ring buffer with valid capcity should be possible {:?}", e),
        };

        // Check if buffer has expected effective capacity.
        crate::ensure_eq!(ring.capacity(), RING_BUFFER_CAPACITY);

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), true);
        crate::ensure_eq!(ring.is_full(), false);

        Ok(ring)
    }

    /// Sequentially enqueues and dequeues elements to/from a ring buffer.
    fn do_enqueue_dequeue(ring: &mut ConcurrentRingBuffer) -> Result<()> {
        // Insert items in the ring buffer.
        let mut push_data: [u8; ITERATIONS] = [0; ITERATIONS];
        for i in 0..ITERATIONS {
            push_data[i] = i as u8;
        }
        let mut elements: usize = 1;
        while elements < ITERATIONS {
            if ring.remaining_capacity() >= elements {
                if let Ok(len) = ring.try_push(&push_data[0..elements]) {
                    crate::ensure_eq!(len, elements);
                    elements = elements + 1;
                } else {
                    anyhow::bail!("Should be able to push");
                }
            } else {
                break;
            };
        }

        trace!("inserted {:?} elements", elements);
        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), false);

        let mut pop_data: [u8; ITERATIONS] = [0; ITERATIONS];
        // Remove items from the ring buffer.
        for i in 1..elements {
            if let Ok(len) = ring.try_pop(&mut pop_data[0..i]) {
                crate::ensure_eq!(len, i);
                crate::ensure_eq!(pop_data[i - 1], i as u8 - 1);
            } else {
                anyhow::bail!("Should be able to pop")
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
        const SIZE: usize = RING_BUFFER_CAPACITY + 2 * mem::size_of::<usize>();
        let mut array: [u8; SIZE] = [0; SIZE];
        do_from_raw(array.as_mut_ptr() as *mut u8, SIZE)?;
        Ok(())
    }

    /// Tets if we succeed to sequentially enqueue and dequeue elements to/from a ring buffer.
    #[test]
    fn enqueue_dequeue_sequential() -> Result<()> {
        let mut ring: ConcurrentRingBuffer = do_new()?;
        do_enqueue_dequeue(&mut ring)
    }

    /// Tets if we succeed to sequentially enqueue and dequeue elements to/from a constructed ring buffer.
    #[test]
    fn enqueue_dequeue_sequential_raw() -> Result<()> {
        const LENGTH: usize = RING_BUFFER_CAPACITY + 2 * mem::size_of::<usize>();
        const SIZE: usize = LENGTH * mem::size_of::<u8>();
        let mut array: [u8; LENGTH] = [0; LENGTH];
        let mut ring: ConcurrentRingBuffer = do_from_raw(array.as_mut_ptr() as *mut u8, SIZE)?;

        do_enqueue_dequeue(&mut ring)
    }

    /// Tests if we succeed to access a ring buffer concurrently.
    #[test]
    fn enqueue_dequeue_concurrent() -> Result<()> {
        const LOG2_NUMBER_OF_THREADS: u8 = 1;
        const NUMBER_OF_THREADS: u8 = 1 << LOG2_NUMBER_OF_THREADS;
        const LOG2_NUMBER_OF_ITERATIONS: u8 = 8 - LOG2_NUMBER_OF_THREADS;
        const NUMBER_OF_ITERATIONS: u8 = (1 << LOG2_NUMBER_OF_ITERATIONS) - 1;
        const BUFFER_SIZE: usize = 16;
        let writer_ring: ConcurrentRingBuffer = do_new()?;
        let reader_ring: ConcurrentRingBuffer = do_new()?;

        // Sanity checks the contents of a message.
        fn check_message(buf: &[u8]) -> Result<()> {
            // Check if all bytes are the same.
            let first_byte = buf[0];
            for byte in buf.iter() {
                crate::ensure_eq!(*byte, first_byte);
            }

            Ok(())
        }

        // Pops a message with a timeout.
        fn pop_message_timeout(ring: &ConcurrentRingBuffer, buf: &mut [u8]) -> Result<usize> {
            let mut max_retries: usize = 8192;
            while max_retries > 0 {
                match ring.try_pop(buf) {
                    Ok(len) => return Ok(len),
                    Err(_) => {
                        max_retries = max_retries - 1;
                        thread::sleep(Duration::from_millis(1));
                    },
                }
            }
            anyhow::bail!("timed out")
        }

        // Pushes a message.
        fn push_message(ring: &ConcurrentRingBuffer, buf: &[u8]) {
            while ring.try_push(buf).is_err() {
                // Retry.
            }
        }

        // Cooks a message.
        fn cook_message(buf: &mut [u8], seqnum: u8, tid: u8) {
            for x in &mut buf[..] {
                *x = (seqnum << LOG2_NUMBER_OF_THREADS) | tid;
            }
        }

        // Parses a message.
        fn parse_message(buf: &[u8]) -> (u8, u8) {
            let seqnum: u8 = buf[0] >> LOG2_NUMBER_OF_THREADS;
            let tid: u8 = buf[0] & ((1 << LOG2_NUMBER_OF_THREADS) - 1);
            (seqnum, tid)
        }

        trace!(
            "starting {} writers and {} readers",
            NUMBER_OF_THREADS / 2,
            NUMBER_OF_THREADS / 2
        );

        thread::scope(|s| {
            let mut writers: Vec<thread::ScopedJoinHandle<()>> = Vec::<thread::ScopedJoinHandle<()>>::new();
            let mut readers: Vec<thread::ScopedJoinHandle<()>> = Vec::<thread::ScopedJoinHandle<()>>::new();

            let range: Range<u8> = 0..NUMBER_OF_THREADS;
            for peer_id in range.step_by(2) {
                let writer_builder: thread::Builder = thread::Builder::new().name(format!("{}", peer_id).to_string());
                let writer: thread::ScopedJoinHandle<()> = writer_builder
                    .spawn_scoped(s, || {
                        let mut seqnum: u8 = 0;
                        let mut buf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                        let self_tid: u8 = thread::current().name().unwrap().parse::<u8>().unwrap();
                        let peer_tid: u8 = self_tid + 1;

                        trace!("writer: started");
                        while seqnum <= NUMBER_OF_ITERATIONS {
                            // Cook message.
                            cook_message(&mut buf[..], seqnum, peer_tid);

                            loop {
                                // Push message.
                                push_message(&writer_ring, &buf[..]);

                                trace!(
                                    "writer: sent (seqnum={:?}/{:?}, tid={:?})",
                                    seqnum,
                                    NUMBER_OF_ITERATIONS,
                                    peer_tid
                                );

                                // Pop message.
                                if pop_message_timeout(&reader_ring, &mut buf[..]).is_err() {
                                    // Timed out, thus start over.
                                    continue;
                                }

                                // Extract peer ID and sequence number.
                                let (recv_seqnum, recv_tid): (u8, u8) = parse_message(&buf[..]);

                                trace!("writer: ack received (seqnum={:?}, tid={})", recv_seqnum, recv_tid);

                                // Check whether or not this thread is the intended recipient for this message.
                                if recv_tid != self_tid {
                                    trace!("writer: dropping message (seqnum={}, bad recipient)", recv_seqnum);
                                    continue;
                                }

                                // Check whether or not if sequence number matches what we expect.
                                if recv_seqnum != seqnum {
                                    trace!("writer: dropping message (seqnum={}, malformed)", seqnum);
                                    continue;
                                }

                                // Check whether or not we received a well-formed message.
                                if check_message(&buf).is_ok() {
                                    // The received received message is good,
                                    // Thus we can break out of the send loop
                                    // and move on to the next message.
                                    break;
                                }
                            }

                            seqnum = seqnum + 1;
                        }
                        trace!("writer: done");
                    })
                    .unwrap();

                let reader_builder: thread::Builder =
                    thread::Builder::new().name(format!("{}", peer_id + 1).to_string());
                let reader: thread::ScopedJoinHandle<()> = reader_builder
                    .spawn_scoped(s, || {
                        let mut next_seqnum: u8 = 0;
                        let mut buf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
                        let self_tid: u8 = thread::current().name().unwrap().parse::<u8>().unwrap();
                        let peer_tid: u8 = self_tid - 1;

                        trace!("reader: started");
                        while next_seqnum <= NUMBER_OF_ITERATIONS {
                            // Pop message.
                            if pop_message_timeout(&writer_ring, &mut buf[..]).is_err() {
                                // Timed out, thus stop.
                                break;
                            }

                            // Extract peer ID and sequence number.
                            let (recv_seqnum, recv_tid): (u8, u8) = parse_message(&buf[..]);

                            trace!("reader: received (seqnum={}, tid={})", recv_seqnum, recv_tid);

                            // Check whether or not this thread is the intended recipient for this message.
                            if recv_tid != self_tid {
                                trace!("reader: dropping message (seqnum={}, bad recipient)", recv_seqnum);
                                continue;
                            }

                            // Check whether or not we received an old message.
                            if recv_seqnum < next_seqnum {
                                trace!("reader: dropping message (seqnum={}, old message)", recv_seqnum);
                                continue;
                            }

                            // Check whether or not we received a malformed message.
                            if check_message(&buf).is_err() {
                                trace!("reader: dropping message (seqnum={}, malformed)", recv_seqnum);
                                continue;
                            }

                            // Check if we.
                            if recv_seqnum == next_seqnum + 1 {
                                next_seqnum = next_seqnum + 1;
                            }

                            // Cook message.
                            cook_message(&mut buf, next_seqnum, peer_tid);

                            // Push message.
                            push_message(&reader_ring, &buf);

                            trace!("reader: ack (seqnum={}, tid={})", next_seqnum, recv_tid);
                        }
                    })
                    .unwrap();
                writers.push(writer);
                readers.push(reader);
            }

            for w in writers {
                w.join().unwrap();
            }
            for r in readers {
                r.join().unwrap();
            }
        });

        Ok(())
    }
}
