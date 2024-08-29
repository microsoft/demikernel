// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    demi_sgarray_t,
    demi_sgaseg_t,
    inetstack::protocols::{
        layer1::PhysicalLayer,
        MAX_HEADER_SIZE,
    },
    runtime::{
        fail::Fail,
        logging,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::consts::RECEIVE_BATCH_SIZE,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::libc::c_void;
use ::std::{
    collections::VecDeque,
    mem,
    ops::{
        Deref,
        DerefMut,
    },
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
    pub fn new_test(now: Instant) -> Self {
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

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

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

impl MemoryRuntime for SharedTestPhysicalLayer {
    /// Allocates a scatter-gather array.
    fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // TODO: Allocate an array of buffers if requested size is too large for a single buffer.

        // We can't allocate a zero-sized buffer.
        if size == 0 {
            let cause: String = format!("cannot allocate a zero-sized buffer");
            error!("sgaalloc(): {}", cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // We can't allocate more than a single buffer.
        if size > u16::MAX as usize {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // First allocate the underlying DemiBuffer.
        let buf: DemiBuffer = DemiBuffer::new_with_headroom(size as u16, MAX_HEADER_SIZE as u16);

        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut c_void,
            sgaseg_len: size as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }
}
