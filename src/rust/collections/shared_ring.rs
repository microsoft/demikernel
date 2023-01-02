// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// TODO: Remove allowances on this module.

//======================================================================================================================
// Imports
//======================================================================================================================
use crate::{
    collections::ring::RingBuffer,
    pal::linux::shm::SharedMemory,
    runtime::fail::Fail,
};
use ::std::ops::Deref;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A ring buffer that may be shared across processes.
///
/// This structure resides on a shared memory region and it is lock-free.
/// This abstraction ensures the correct concurrent access by a single writer and a single reader.
pub struct SharedRingBuffer<T: Copy> {
    #[allow(unused)]
    shm: SharedMemory,
    ring: RingBuffer<T>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for shared ring buffers.
impl<T: Copy> SharedRingBuffer<T> {
    /// Creates a new shared ring buffer.
    pub fn create(name: &str, capacity: usize) -> Result<SharedRingBuffer<T>, Fail> {
        let mut shm: SharedMemory = SharedMemory::create(&name, capacity)?;
        let ring: RingBuffer<T> = RingBuffer::<T>::from_raw_parts(true, shm.as_mut_ptr(), shm.len())?;
        Ok(SharedRingBuffer { shm, ring })
    }

    /// Opens an existing shared ring buffer.
    pub fn open(name: &str, capacity: usize) -> Result<SharedRingBuffer<T>, Fail> {
        let mut shm: SharedMemory = SharedMemory::open(&name, capacity)?;
        let ring: RingBuffer<T> = RingBuffer::<T>::from_raw_parts(false, shm.as_mut_ptr(), shm.len())?;
        Ok(SharedRingBuffer { shm, ring })
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Dereference trait implementation for shared ring buffers.
impl<T: Copy> Deref for SharedRingBuffer<T> {
    type Target = RingBuffer<T>;

    fn deref(&self) -> &Self::Target {
        &self.ring
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use super::SharedRingBuffer;
    use std::{
        thread::{
            self,
            ScopedJoinHandle,
        },
        time::Duration,
    };

    const RING_BUFFER_CAPACITY: usize = 4096;

    /// Tests if we succeed to perform sequential accesses to a shared ring buffer.
    #[ignore]
    #[test]
    fn ring_buffer_on_shm_sequential() {
        let shm_name: String = "shm-test-ring-buffer-serial".to_string();
        let ring: SharedRingBuffer<u8> = match SharedRingBuffer::<u8>::create(&shm_name, RING_BUFFER_CAPACITY) {
            Ok(ring) => ring,
            Err(_) => panic!("creating a shared ring buffer should be possible"),
        };

        for i in 0..ring.capacity() {
            ring.enqueue((i & 255) as u8);
        }

        // Check if buffer state is consistent.
        assert!(ring.is_empty() == false);
        assert!(ring.is_full() == true);

        // Remove items from the ring buffer.
        for i in 0..ring.capacity() {
            let item: u8 = ring.dequeue();
            assert!(item == (i & 255) as u8);
        }

        // Check if buffer state is consistent.
        assert!(ring.is_empty() == true);
        assert!(ring.is_full() == false);
    }

    /// Tests if we succeed to perform concurrent accesses to a shared ring buffer..
    #[ignore]
    #[test]
    fn ring_buffer_on_shm_concurrent() {
        let shm_name: String = "shm-test-ring-buffer-concurrent".to_string();

        thread::scope(|s| {
            let writer: ScopedJoinHandle<()> = s.spawn(|| {
                let ring: SharedRingBuffer<u8> = match SharedRingBuffer::<u8>::create(&shm_name, RING_BUFFER_CAPACITY) {
                    Ok(ring) => ring,
                    Err(_) => panic!("creating a shared ring buffer should be possible"),
                };

                for i in 0..ring.capacity() {
                    ring.enqueue((i & 255) as u8);
                }

                while !ring.is_empty() {}
            });

            let reader: ScopedJoinHandle<()> = s.spawn(|| {
                thread::sleep(Duration::from_millis(100));

                let ring: SharedRingBuffer<u8> = match SharedRingBuffer::<u8>::open(&shm_name, RING_BUFFER_CAPACITY) {
                    Ok(ring) => ring,
                    Err(_) => panic!("openining a shared ring buffer should be possible"),
                };
                for i in 0..ring.capacity() {
                    let item: u8 = ring.dequeue();
                    assert!(item == (i & 255) as u8);
                }
            });

            writer.join().unwrap();
            reader.join().unwrap();
        });
    }
}
