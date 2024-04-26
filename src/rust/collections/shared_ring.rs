// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// TODO: Remove allowances on this module.

//======================================================================================================================
// Imports
//======================================================================================================================
use crate::{
    collections::ring::Ring,
    pal::linux::shm::SharedMemory,
    runtime::fail::Fail,
};
use ::std::ops::{
    Deref,
    DerefMut,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A ring buffer that may be shared across processes.
///
/// This structure resides on a shared memory region and it is lock-free.
/// This abstraction ensures the correct concurrent access by a single writer and a single reader.
pub struct SharedRingBuffer<T: Ring> {
    #[allow(unused)]
    shm: SharedMemory,
    ring: T,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for shared ring buffers.
impl<T: Ring> SharedRingBuffer<T> {
    /// Creates a new shared ring buffer.
    pub fn create(name: &str, capacity: usize) -> Result<Self, Fail> {
        let mut shm: SharedMemory = SharedMemory::create(&name, capacity)?;
        let ring: T = T::from_raw_parts(true, shm.as_mut_ptr(), shm.len())?;
        Ok(SharedRingBuffer { shm, ring })
    }

    /// Opens an existing shared ring buffer.
    pub fn open(name: &str, capacity: usize) -> Result<Self, Fail> {
        let mut shm: SharedMemory = SharedMemory::open(&name, capacity)?;
        let ring: T = T::from_raw_parts(false, shm.as_mut_ptr(), shm.len())?;
        Ok(SharedRingBuffer { shm, ring })
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Dereference trait implementation for shared ring buffers.
impl<T: Ring> Deref for SharedRingBuffer<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.ring
    }
}

impl<T: Ring> DerefMut for SharedRingBuffer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ring
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use crate::collections::{
        ring::RingBuffer,
        shared_ring::SharedRingBuffer,
    };
    use ::anyhow::Result;
    use ::std::{
        sync::Barrier,
        thread::{
            self,
            ScopedJoinHandle,
        },
    };

    /// Number of rounds to run the tests.
    const ROUNDS: usize = 128;
    /// Capacity of ring buffers used in tests.
    const RING_BUFFER_CAPACITY: usize = 4096;

    /// Tests if we succeed to perform sequential accesses to a shared ring buffer.
    #[test]
    fn ring_buffer_on_shm_sequential() -> Result<()> {
        let shm_name: String = "shm-test-ring-buffer-serial".to_string();
        let mut ring: SharedRingBuffer<RingBuffer<u8>> =
            match SharedRingBuffer::<RingBuffer<u8>>::create(&shm_name, RING_BUFFER_CAPACITY) {
                Ok(ring) => ring,
                Err(e) => anyhow::bail!("creating a shared ring buffer should be possible: {}", e.to_string()),
            };

        let capacity: usize = ring.capacity();

        {
            let (mut producer, _) = ring.open();

            for i in 0..capacity {
                producer.try_enqueue((i & 255) as u8)?;
            }
        }

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), false);
        crate::ensure_eq!(ring.is_full(), true);

        {
            // Remove items from the ring buffer.
            let (_, mut consumer) = ring.open();
            for i in 0..capacity {
                let item: u8 = consumer.try_dequeue()?;
                crate::ensure_eq!(item, (i & 255) as u8);
            }
        }

        // Check if buffer state is consistent.
        crate::ensure_eq!(ring.is_empty(), true);
        crate::ensure_eq!(ring.is_full(), false);

        Ok(())
    }

    /// Tests if we succeed to perform concurrent accesses to a shared ring buffer..
    #[test]
    fn ring_buffer_on_shm_concurrent() -> Result<()> {
        let shm_name: String = "shm-test-ring-buffer-concurrent".to_string();
        let mut result: Result<()> = Ok(());
        let barrier: Barrier = Barrier::new(2);

        thread::scope(|s| {
            let writer: ScopedJoinHandle<Result<()>> = s.spawn(|| {
                let mut ring: SharedRingBuffer<RingBuffer<u8>> =
                    match SharedRingBuffer::<RingBuffer<u8>>::create(&shm_name, RING_BUFFER_CAPACITY) {
                        Ok(ring) => ring,
                        Err(_) => anyhow::bail!("creating a shared ring buffer should be possible"),
                    };

                let capacity = ring.capacity();
                let (mut producer, _) = ring.open();

                barrier.wait();

                for _ in 0..ROUNDS {
                    for i in 0..capacity {
                        while let Err(_) = producer.try_enqueue((i & 255) as u8) {}
                    }
                }

                while !ring.is_empty() {}
                Ok(())
            });

            let reader: ScopedJoinHandle<Result<()>> = s.spawn(|| {
                barrier.wait();

                let mut ring: SharedRingBuffer<RingBuffer<u8>> =
                    match SharedRingBuffer::<RingBuffer<u8>>::open(&shm_name, RING_BUFFER_CAPACITY) {
                        Ok(ring) => ring,
                        Err(_) => anyhow::bail!("opening a shared ring buffer should be possible"),
                    };
                let capacity = ring.capacity();
                let (_, mut consumer) = ring.open();
                for _ in 0..ROUNDS {
                    for i in 0..capacity {
                        let item: u8 = loop {
                            match consumer.try_dequeue() {
                                Ok(item) => break item,
                                Err(_) => (),
                            }
                        };
                        crate::ensure_eq!(item, (i & 255) as u8);
                    }
                }
                Ok(())
            });

            result = writer.join().unwrap().and(reader.join().unwrap());
        });

        result
    }
}
