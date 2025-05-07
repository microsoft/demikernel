// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::consts::{MAX_HEADER_SIZE, RECEIVE_BATCH_SIZE},
    inetstack::protocols::layer1::PhysicalLayer,
    runtime::{
        fail::Fail,
        logging,
        memory::{DemiBuffer, DemiMemoryAllocator},
        SharedDemiRuntime, SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    time::Instant,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct TestPhysicalLayer {
    incoming: VecDeque<DemiBuffer>,
    outgoing: VecDeque<DemiBuffer>,
    runtime: SharedDemiRuntime,
}

#[derive(Clone)]
pub struct SharedTestPhysicalLayer(SharedObject<TestPhysicalLayer>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl SharedTestPhysicalLayer {
    pub fn new(now: Instant) -> Self {
        logging::initialize();
        Self(SharedObject::<TestPhysicalLayer>::new(TestPhysicalLayer {
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
            runtime: SharedDemiRuntime::new(now),
        }))
    }

    /// Remove a fixed number of frames from the runtime's outgoing queue.
    fn pop_frames(&mut self, num_frames: usize) -> VecDeque<DemiBuffer> {
        let length: usize = self.outgoing.len();
        self.outgoing.split_off(length - num_frames)
    }

    pub fn pop_all_frames(&mut self) -> VecDeque<DemiBuffer> {
        self.outgoing.split_off(0)
    }

    /// Remove a single frame from the runtime's outgoing queue. The queue should not be empty.
    pub fn pop_frame(&mut self) -> DemiBuffer {
        self.pop_frames(1).pop_front().expect("should be at least one frame")
    }

    pub fn push_frame(&mut self, pkt: DemiBuffer) {
        self.incoming.push_back(pkt);
    }

    /// Get the underlying DemiRuntime.
    pub fn get_runtime(&self) -> SharedDemiRuntime {
        self.runtime.clone()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl PhysicalLayer for SharedTestPhysicalLayer {
    fn transmit(&mut self, pkt: DemiBuffer) -> Result<(), Fail> {
        debug!(
            "transmit frame: {:?} total packet size: {:?}",
            self.outgoing.len(),
            pkt.len()
        );

        // The packet header and body must fit into whatever physical media we're transmitting over.
        // For this test harness, we 2^16 bytes (u16::MAX) as our limit.
        assert!(pkt.len() < u16::MAX as usize);

        self.outgoing.push_back(pkt);
        Ok(())
    }

    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.incoming.pop_front() {
            out.push(buf);
        }
        Ok(out)
    }
}

impl Deref for SharedTestPhysicalLayer {
    type Target = TestPhysicalLayer;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedTestPhysicalLayer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl DemiMemoryAllocator for SharedTestPhysicalLayer {
    /// Allocates a scatter-gather array.
    fn allocate_demi_buffer(&self, size: usize) -> Result<DemiBuffer, Fail> {
        Ok(DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16))
    }
}
