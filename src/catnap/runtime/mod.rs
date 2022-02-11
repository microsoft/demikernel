// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod memory;
mod network;
mod scheduler;
mod utils;

//==============================================================================
// Imports
//==============================================================================

use self::network::{
    raw_sockaddr,
    SockAddrPurpose,
    ETH_P_ALL,
};
use ::anyhow::Error;
use ::catnip::timer::{
    Timer,
    TimerRc,
};
use ::catwalk::Scheduler;
use ::libc;
use ::rand::{
    rngs::SmallRng,
    SeedableRng,
};
use ::runtime::{
    network::{
        config::{
            ArpConfig,
            TcpConfig,
        },
        types::MacAddress,
    },
    Runtime,
};
use ::socket2::{
    Domain,
    Socket,
    Type,
};
use ::std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    net::Ipv4Addr,
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Constants & Structures
//==============================================================================

#[derive(Clone)]
pub struct LinuxRuntime {
    inner: Rc<RefCell<Inner>>,
    scheduler: Scheduler,
}

pub struct Inner {
    pub timer: TimerRc,
    pub rng: SmallRng,
    pub socket: Socket,
    pub ifindex: i32,
    pub link_addr: MacAddress,
    pub ipv4_addr: Ipv4Addr,
    pub tcp_options: TcpConfig,
    pub arp_options: ArpConfig,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl LinuxRuntime {
    pub fn new(
        now: Instant,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        interface_name: &str,
        arp: HashMap<Ipv4Addr, MacAddress>,
    ) -> Self {
        let arp_options: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(arp),
            Some(false),
        );

        let socket = Socket::new(
            Domain::PACKET,
            Type::RAW.nonblocking(),
            Some((ETH_P_ALL as libc::c_int).into()),
        )
        .unwrap();
        let path: String = format!("/sys/class/net/{}/ifindex", interface_name);
        let ifindex: i32 = fs::read_to_string(path)
            .expect("Could not read ifindex")
            .trim()
            .parse()
            .unwrap();

        socket
            .bind(&raw_sockaddr(SockAddrPurpose::Bind, ifindex, &[0; 6]))
            .unwrap();

        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            rng: SmallRng::from_seed([0; 32]),
            socket,
            ifindex,
            link_addr,
            ipv4_addr,
            tcp_options: TcpConfig::default(),
            arp_options,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::default(),
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Runtime for LinuxRuntime {}

//==============================================================================
// Helper Functions
//==============================================================================

pub fn initialize_linux(
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    interface_name: &str,
    arp_table: HashMap<Ipv4Addr, MacAddress>,
) -> Result<LinuxRuntime, Error> {
    Ok(LinuxRuntime::new(
        Instant::now(),
        local_link_addr,
        local_ipv4_addr,
        interface_name,
        arp_table,
    ))
}
