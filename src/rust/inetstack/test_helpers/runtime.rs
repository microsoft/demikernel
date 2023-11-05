// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::runtime::{
    logging,
    memory::DemiBuffer,
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        types::MacAddress,
        NetworkRuntime,
        PacketBuf,
    },
    timer::SharedTimer,
    SharedDemiRuntime,
    SharedObject,
};
use ::arrayvec::ArrayVec;
use ::std::{
    collections::VecDeque,
    net::Ipv4Addr,
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
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    arp_config: ArpConfig,
    udp_config: UdpConfig,
    tcp_config: TcpConfig,
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
    pub fn new(
        now: Instant,
        arp_config: ArpConfig,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
    ) -> Self {
        logging::initialize();
        Self(SharedObject::<TestRuntime>::new(TestRuntime {
            link_addr,
            ipv4_addr,
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
            runtime: SharedDemiRuntime::new(now),
            arp_config,
            udp_config,
            tcp_config,
        }))
    }

    /// Remove a fixed number of frames from the runtime's outgoing queue.
    pub fn pop_frames(&mut self, num_frames: usize) -> VecDeque<DemiBuffer> {
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

    /// Remove a single frame from the runtime's outgoing queue if it is not empty.
    pub fn pop_frame_unchecked(&mut self) -> Option<DemiBuffer> {
        self.pop_frames(1).pop_front()
    }

    /// Add a frame to the runtime's incoming queue.
    pub fn push_frame(&mut self, buf: DemiBuffer) {
        self.incoming.push_back(buf);
    }

    pub fn poll_scheduler(&mut self) {
        self.runtime.poll();
    }

    /// Get the link address assigned to the runtime.
    pub fn get_link_addr(&self) -> MacAddress {
        self.link_addr
    }

    /// Get the ip address assigned to the runtime.
    pub fn get_ip_addr(&self) -> Ipv4Addr {
        self.ipv4_addr
    }

    /// Get the arp configuration options for the runtime.
    pub fn get_arp_config(&self) -> ArpConfig {
        self.arp_config.clone()
    }

    /// Get the udp configuration options for the runtime.
    pub fn get_udp_config(&self) -> UdpConfig {
        self.udp_config.clone()
    }

    /// Get the tcp configuration options for the runtime.
    pub fn get_tcp_config(&self) -> TcpConfig {
        self.tcp_config.clone()
    }

    /// Get the runtime's clock.
    pub fn get_timer(&self) -> SharedTimer {
        self.runtime.get_timer()
    }

    /// Advance runtime's clock
    pub fn advance_clock(&mut self, now: Instant) {
        self.runtime.advance_clock(now)
    }

    /// Get the underlying DemiRuntime.
    pub fn get_runtime(&self) -> SharedDemiRuntime {
        self.runtime.clone()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl<const N: usize> NetworkRuntime<N> for SharedTestRuntime {
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

    fn receive(&mut self) -> ArrayVec<DemiBuffer, N> {
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
