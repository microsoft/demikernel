// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        logging,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::{
            consts::RECEIVE_BATCH_SIZE,
            NetworkRuntime,
            PacketBuf,
        },
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::std::{
    collections::VecDeque,
    ops::{
        Deref,
        DerefMut,
    },
    time::Instant,
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct TestRuntime {
    incoming: VecDeque<DemiBuffer>,
    outgoing: VecDeque<DemiBuffer>,
    runtime: SharedDemiRuntime,
}

#[derive(Clone)]
pub struct SharedTestRuntime(SharedObject<TestRuntime>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl SharedTestRuntime {
    pub fn new_test(now: Instant) -> Self {
        logging::initialize();
        Self(SharedObject::<TestRuntime>::new(TestRuntime {
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

    /// Get the underlying DemiRuntime.
    pub fn get_runtime(&self) -> SharedDemiRuntime {
        self.runtime.clone()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl NetworkRuntime for SharedTestRuntime {
    fn new(_config: &Config) -> Result<Self, Fail> {
        logging::initialize();
        Ok(Self(SharedObject::<TestRuntime>::new(TestRuntime {
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
            runtime: SharedDemiRuntime::new(Instant::now()),
        })))
    }

    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();
        debug!("transmit frame: {:?} body: {:?}", self.outgoing.len(), body_size);

        // The packet header and body must fit into whatever physical media we're transmitting over.
        // For this test harness, we 2^16 bytes (u16::MAX) as our limit.
        assert!(header_size + body_size < u16::MAX as usize);

        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);
        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }
        self.outgoing.push_back(buf);
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.incoming.pop_front() {
            out.push(buf);
        }
        out
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedTestRuntime {
    type Target = TestRuntime;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedTestRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl MemoryRuntime for SharedTestRuntime {}
