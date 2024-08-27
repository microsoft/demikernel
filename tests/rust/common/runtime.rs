// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::arrayvec::ArrayVec;
use ::demikernel::{
    demi_sgarray_t,
    demi_sgaseg_t,
    demikernel::config::Config,
    inetstack::protocols::{
        layer1::{
            PacketBuf,
            PhysicalLayer,
        },
        MAX_HEADER_SIZE,
    },
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
            MemoryRuntime,
        },
        network::consts::RECEIVE_BATCH_SIZE,
        SharedObject,
    },
};
use ::libc::c_void;
use ::log::{
    error,
    warn,
};
use ::std::{
    mem,
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Dummy Runtime
pub struct DummyRuntime {
    /// Shared Member Fields
    /// Random Number Generator
    /// Incoming Queue of Packets
    incoming: crossbeam_channel::Receiver<DemiBuffer>,
    /// Outgoing Queue of Packets
    outgoing: crossbeam_channel::Sender<DemiBuffer>,
}

#[derive(Clone)]

/// Shared Dummy Runtime
pub struct SharedDummyRuntime(SharedObject<DummyRuntime>);

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Dummy Runtime
impl SharedDummyRuntime {
    /// Creates a Dummy Runtime.
    pub fn new(
        incoming: crossbeam_channel::Receiver<DemiBuffer>,
        outgoing: crossbeam_channel::Sender<DemiBuffer>,
    ) -> Self {
        Self(SharedObject::new(DummyRuntime { incoming, outgoing }))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Network Runtime Trait Implementation for Dummy Runtime
impl PhysicalLayer for SharedDummyRuntime {
    /// Creates a Dummy Runtime.
    fn new(_config: &Config) -> Result<Self, Fail> {
        Err(Fail::new(
            libc::ENOTSUP,
            "this function is not supported for the dummy runtime",
        ))
    }

    fn transmit<P: PacketBuf>(&mut self, mut pkt: P) -> Result<(), Fail> {
        let outgoing_pkt: DemiBuffer = match pkt.take_body() {
            Some(body) => body,
            _ => {
                let cause = format!("No body in PacketBuf to transmit");
                warn!("{}", cause);
                return Err(Fail::new(libc::EINVAL, &cause));
            },
        };
        // The packet header and body must fit into whatever physical media we're transmitting over.
        // For this test harness, we 2^16 bytes (u16::MAX) as our limit.
        assert!(outgoing_pkt.len() < u16::MAX as usize);

        match self.outgoing.try_send(outgoing_pkt) {
            Ok(_) => Ok(()),
            Err(_) => Err(Fail::new(
                libc::EAGAIN,
                "Could not push outgoing packet to the shared channel",
            )),
        }
    }

    fn receive(&mut self) -> Result<ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE>, Fail> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.incoming.try_recv().ok() {
            out.push(buf);
        }
        Ok(out)
    }
}

impl MemoryRuntime for SharedDummyRuntime {
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
        // Always allocate with header space for now even if we do not need it.
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

impl Deref for SharedDummyRuntime {
    type Target = DummyRuntime;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedDummyRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
