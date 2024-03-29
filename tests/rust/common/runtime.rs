// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::arrayvec::ArrayVec;
use ::demikernel::runtime::{
    memory::{
        DemiBuffer,
        MemoryRuntime,
    },
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        consts::RECEIVE_BATCH_SIZE,
        NetworkRuntime,
        PacketBuf,
    },
    SharedObject,
};
use ::std::ops::{
    Deref,
    DerefMut,
};

//==============================================================================
// Structures
//==============================================================================

/// Dummy Runtime
pub struct DummyRuntime {
    /// Shared Member Fields
    /// Random Number Generator
    /// Incoming Queue of Packets
    incoming: crossbeam_channel::Receiver<DemiBuffer>,
    /// Outgoing Queue of Packets
    outgoing: crossbeam_channel::Sender<DemiBuffer>,
    /// ARP config.
    arp_config: ArpConfig,
    /// TCP config.
    tcp_config: TcpConfig,
    /// UDP config.
    udp_config: UdpConfig,
}

#[derive(Clone)]

/// Shared Dummy Runtime
pub struct SharedDummyRuntime(SharedObject<DummyRuntime>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Dummy Runtime
impl SharedDummyRuntime {
    /// Creates a Dummy Runtime.
    pub fn new(
        incoming: crossbeam_channel::Receiver<DemiBuffer>,
        outgoing: crossbeam_channel::Sender<DemiBuffer>,
        arp_config: ArpConfig,
        tcp_config: TcpConfig,
        udp_config: UdpConfig,
    ) -> Self {
        Self(SharedObject::new(DummyRuntime {
            incoming,
            outgoing,
            arp_config,
            tcp_config,
            udp_config,
        }))
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for Dummy Runtime
impl NetworkRuntime for SharedDummyRuntime {
    fn transmit(&mut self, pkt: Box<dyn PacketBuf>) {
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
        self.outgoing.try_send(buf).unwrap();
    }

    fn receive(&mut self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.incoming.try_recv().ok() {
            out.push(buf);
        }
        out
    }

    fn get_arp_config(&self) -> ArpConfig {
        self.arp_config.clone()
    }

    fn get_tcp_config(&self) -> TcpConfig {
        self.tcp_config.clone()
    }

    fn get_udp_config(&self) -> UdpConfig {
        self.udp_config.clone()
    }
}

impl MemoryRuntime for SharedDummyRuntime {}

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
