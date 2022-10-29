// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::arrayvec::ArrayVec;
use ::crossbeam_channel;
use ::demikernel::{
    runtime::{
        memory::DemiBuffer,
        network::{
            consts::RECEIVE_BATCH_SIZE,
            NetworkRuntime,
            PacketBuf,
        },
        timer::{
            Timer,
            TimerRc,
        },
    },
    scheduler::scheduler::Scheduler,
};
use ::std::{
    cell::RefCell,
    rc::Rc,
    time::Instant,
};

//==============================================================================
// Structures
//==============================================================================

/// Shared Dummy Runtime
struct SharedDummyRuntime {
    /// Random Number Generator
    /// Incoming Queue of Packets
    incoming: crossbeam_channel::Receiver<DemiBuffer>,
    /// Outgoing Queue of Packets
    outgoing: crossbeam_channel::Sender<DemiBuffer>,
}

/// Dummy Runtime
#[derive(Clone)]
pub struct DummyRuntime {
    /// Shared Member Fields
    inner: Rc<RefCell<SharedDummyRuntime>>,
    pub scheduler: Scheduler,
    pub clock: TimerRc,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Dummy Runtime
impl DummyRuntime {
    /// Creates a Dummy Runtime.
    pub fn new(
        now: Instant,
        incoming: crossbeam_channel::Receiver<DemiBuffer>,
        outgoing: crossbeam_channel::Sender<DemiBuffer>,
    ) -> Self {
        let inner = SharedDummyRuntime { incoming, outgoing };
        Self {
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::default(),
            clock: TimerRc(Rc::new(Timer::new(now))),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for Dummy Runtime
impl NetworkRuntime for DummyRuntime {
    fn transmit(&self, pkt: Box<dyn PacketBuf>) {
        let header_size: usize = pkt.header_size();
        let body_size: usize = pkt.body_size();

        // The packet header and body must fit into whatever physical media we're transmitting over.
        // For this test harness, we 2^16 bytes (u16::MAX) as our limit.
        assert!(header_size + body_size < u16::MAX as usize);

        let mut buf: DemiBuffer = DemiBuffer::new((header_size + body_size) as u16);
        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }
        self.inner.borrow_mut().outgoing.try_send(buf).unwrap();
    }

    fn receive(&self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.inner.borrow_mut().incoming.try_recv().ok() {
            out.push(buf);
        }
        out
    }
}
