// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::runtime::DummyRuntime;
use ::demikernel::{
    inetstack::InetStack,
    runtime::{
        logging,
        memory::{
            Buffer,
            DataBuffer,
        },
        network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            types::MacAddress,
        },
        scheduler::scheduler::Scheduler,
        timer::TimerRc,
    },
};
use crossbeam_channel::{
    Receiver,
    Sender,
};
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Structures
//==============================================================================

pub struct DummyLibOS {}

//==============================================================================
// Associated Functons
//==============================================================================

impl DummyLibOS {
    /// Initializes the libOS.
    pub fn new(
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        tx: Sender<DataBuffer>,
        rx: Receiver<DataBuffer>,
        arp: HashMap<Ipv4Addr, MacAddress>,
    ) -> InetStack {
        let now: Instant = Instant::now();
        let rt: Rc<DummyRuntime> = Rc::new(DummyRuntime::new(now, rx, tx));
        let arp_options: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(arp.clone()),
            Some(false),
        );
        let udp_config: UdpConfig = UdpConfig::default();
        let tcp_config: TcpConfig = TcpConfig::default();
        let scheduler: Scheduler = rt.scheduler.clone();
        let clock: TimerRc = rt.clock.clone();
        let rng_seed: [u8; 32] = [0; 32];
        logging::initialize();
        InetStack::new(
            rt,
            scheduler,
            clock,
            link_addr,
            ipv4_addr,
            udp_config,
            tcp_config,
            rng_seed,
            arp_options,
        )
        .unwrap()
    }

    /// Cooks a buffer.
    pub fn cook_data(size: usize) -> Buffer {
        let fill_char: u8 = b'a';

        let mut buf: Buffer = Buffer::Heap(DataBuffer::new(size).unwrap());
        for a in &mut buf[..] {
            *a = fill_char;
        }
        buf
    }
}
