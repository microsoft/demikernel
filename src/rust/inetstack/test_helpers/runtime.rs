// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    runtime::{
        logging,
        memory::DemiBuffer,
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            consts::RECEIVE_BATCH_SIZE,
            types::MacAddress,
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
use ::arrayvec::ArrayVec;
use ::std::{
    cell::RefCell,
    collections::VecDeque,
    net::Ipv4Addr,
    rc::Rc,
    time::Instant,
};

//==============================================================================
// Structures
//==============================================================================

// TODO: Drop inner mutability pattern.
pub struct Inner {
    #[allow(unused)]
    timer: TimerRc,
    incoming: VecDeque<DemiBuffer>,
    outgoing: VecDeque<DemiBuffer>,
}

#[derive(Clone)]
pub struct TestRuntime {
    pub link_addr: MacAddress,
    pub ipv4_addr: Ipv4Addr,
    pub arp_options: ArpConfig,
    pub udp_config: UdpConfig,
    pub tcp_config: TcpConfig,
    inner: Rc<RefCell<Inner>>,
    pub scheduler: Scheduler,
    pub clock: TimerRc,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl TestRuntime {
    pub fn new(
        now: Instant,
        arp_options: ArpConfig,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
    ) -> Self {
        logging::initialize();

        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
        };
        Self {
            link_addr,
            ipv4_addr,
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::default(),
            clock: TimerRc(Rc::new(Timer::new(now))),
            arp_options,
            udp_config,
            tcp_config,
        }
    }

    pub fn pop_frame(&self) -> DemiBuffer {
        self.inner
            .borrow_mut()
            .outgoing
            .pop_front()
            .expect("pop_front didn't return an outgoing frame")
    }

    pub fn pop_frame_unchecked(&self) -> Option<DemiBuffer> {
        self.inner.borrow_mut().outgoing.pop_front()
    }

    pub fn push_frame(&self, buf: DemiBuffer) {
        self.inner.borrow_mut().incoming.push_back(buf);
    }

    pub fn poll_scheduler(&self) {
        // let mut ctx = Context::from_waker(noop_waker_ref());
        self.scheduler.poll();
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl NetworkRuntime for TestRuntime {
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
        self.inner.borrow_mut().outgoing.push_back(buf);
    }

    fn receive(&self) -> ArrayVec<DemiBuffer, RECEIVE_BATCH_SIZE> {
        let mut out = ArrayVec::new();
        if let Some(buf) = self.inner.borrow_mut().incoming.pop_front() {
            out.push(buf);
        }
        out
    }
}
