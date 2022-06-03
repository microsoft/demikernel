// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod memory;
mod network;
mod rawsocket;
mod scheduler;
mod utils;

//==============================================================================
// Imports
//==============================================================================

use self::rawsocket::{
    RawSocket,
    RawSocketAddr,
};
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
    scheduler::Scheduler,
    timer::{
        Timer,
        TimerRc,
    },
    Runtime,
};
use ::std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    net::Ipv4Addr,
    num::ParseIntError,
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Constants & Structures
//==============================================================================

/// Linux Runtime
#[derive(Clone)]
pub struct LinuxRuntime {
    timer: TimerRc,
    scheduler: Scheduler,
    tcp_options: TcpConfig,
    udp_options: UdpConfig,
    arp_options: ArpConfig,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    ifindex: i32,
    socket: Rc<RefCell<RawSocket>>,
    rng: Rc<RefCell<SmallRng>>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Linux Runtime
impl LinuxRuntime {
    /// Instantiates a Linux Runtime.
    pub fn new(
        now: Instant,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        ifname: &str,
        arp: HashMap<Ipv4Addr, MacAddress>,
    ) -> Self {
        let arp_options: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(arp),
            Some(false),
        );

        // TODO: Make this constructor return a Result and drop expect() calls bellow.
        let mac_addr: [u8; 6] = [0; 6];
        let ifindex: i32 = Self::get_ifindex(ifname).expect("could not parse ifindex");
        let socket: RawSocket = RawSocket::new().expect("could not create raw socket");
        let sockaddr: RawSocketAddr = RawSocketAddr::new(ifindex, &mac_addr);
        socket.bind(&sockaddr).expect("could not bind raw socket");

        Self {
            scheduler: Scheduler::default(),
            timer: TimerRc(Rc::new(Timer::new(now))),
            tcp_options: TcpConfig::default(),
            udp_options: UdpConfig::default(),
            arp_options,
            link_addr,
            ipv4_addr,
            ifindex,
            socket: Rc::new(RefCell::new(socket)),
            rng: Rc::new(RefCell::new(SmallRng::from_seed([0; 32]))),
        }
    }

    /// Gets the interface index of the network interface named `ifname`.
    fn get_ifindex(ifname: &str) -> Result<i32, ParseIntError> {
        let path: String = format!("/sys/class/net/{}/ifindex", ifname);
        fs::read_to_string(path).expect("could not read ifname").trim().parse()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Runtime Trait Implementation for Linux Runtime
impl Runtime for LinuxRuntime {}
