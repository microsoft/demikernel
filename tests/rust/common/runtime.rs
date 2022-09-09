// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::demikernel::runtime::{
    memory::{
        Buffer,
        DataBuffer,
    },
    network::{
        consts::RECEIVE_BATCH_SIZE,
        NetworkRuntime,
        PacketBuf,
    },
    scheduler::scheduler::Scheduler,
    timer::{
        Timer,
        TimerRc,
    },
};
use arrayvec::ArrayVec;
use crossbeam_channel;
use std::{
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
    incoming: crossbeam_channel::Receiver<DataBuffer>,
    /// Outgoing Queue of Packets
    outgoing: crossbeam_channel::Sender<DataBuffer>,
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
        incoming: crossbeam_channel::Receiver<DataBuffer>,
        outgoing: crossbeam_channel::Sender<DataBuffer>,
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
        let header_size = pkt.header_size();
        let body_size = pkt.body_size();

        let mut buf: DataBuffer = DataBuffer::new(header_size + body_size).unwrap();
        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }
        self.inner.borrow_mut().outgoing.try_send(buf).unwrap();
    }

    fn receive(&self) -> ArrayVec<Buffer, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.inner.borrow_mut().incoming.try_recv().ok() {
            let dbuf: Buffer = Buffer::Heap(buf);
            out.push(dbuf);
        }
        out
    }
}
