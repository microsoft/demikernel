// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod memory;
mod network;
mod scheduler;
mod utils;

//==============================================================================
// Imports
//==============================================================================

use self::scheduler::TimerRc;
use crate::catnip::memory::{
    Mbuf,
    MemoryManager,
};
use ::catnip::timer::Timer;
use ::catwalk::Scheduler;
use ::rand::{
    rngs::SmallRng,
    SeedableRng,
};
use ::runtime::{
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        types::MacAddress,
    },
    Runtime,
};
use ::std::{
    cell::RefCell,
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

struct Inner {
    timer: TimerRc,
    memory_manager: MemoryManager,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    rng: SmallRng,
    arp_options: ArpConfig,
    tcp_options: TcpConfig,
    udp_options: UdpConfig,

    dpdk_port_id: u16,
}

#[derive(Clone)]
pub struct DPDKRuntime {
    inner: Rc<RefCell<Inner>>,
    scheduler: Scheduler,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl DPDKRuntime {
    pub fn new(
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        dpdk_port_id: u16,
        memory_manager: MemoryManager,
        arp_table: HashMap<Ipv4Addr, MacAddress>,
        disable_arp: bool,
        mss: usize,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let rng = SmallRng::from_rng(&mut rng).expect("Failed to initialize RNG");
        let now = Instant::now();

        let arp_options = ArpConfig::new(
            Some(Duration::from_secs(15)),
            Some(Duration::from_secs(20)),
            Some(5),
            Some(arp_table),
            Some(disable_arp),
        );

        let tcp_options = TcpConfig::new(
            Some(mss),
            None,
            None,
            Some(0xffff),
            Some(0),
            None,
            Some(tcp_checksum_offload),
            Some(tcp_checksum_offload),
        );

        let udp_options = UdpConfig::new(Some(udp_checksum_offload), Some(udp_checksum_offload));

        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            link_addr,
            ipv4_addr,
            rng,
            arp_options,
            tcp_options,
            udp_options,

            dpdk_port_id,
            memory_manager,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::default(),
        }
    }

    pub fn alloc_body_mbuf(&self) -> Mbuf {
        self.inner.borrow().memory_manager.alloc_body_mbuf()
    }

    pub fn port_id(&self) -> u16 {
        self.inner.borrow().dpdk_port_id
    }

    pub fn memory_manager(&self) -> MemoryManager {
        self.inner.borrow().memory_manager.clone()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Runtime for DPDKRuntime {}
