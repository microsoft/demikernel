// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod network;
mod rawsocket;

//==============================================================================
// Imports
//==============================================================================

use self::rawsocket::{
    RawSocket,
    RawSocketAddr,
};
use crate::runtime::{
    memory::MemoryRuntime,
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
    fs,
    net::Ipv4Addr,
    num::ParseIntError,
    rc::Rc,
    time::Duration,
};

//==============================================================================
// Constants & Structures
//==============================================================================

/// Linux Runtime
#[derive(Clone)]
pub struct LinuxRuntime {
    pub tcp_options: TcpConfig,
    pub udp_options: UdpConfig,
    pub arp_options: ArpConfig,
    pub link_addr: MacAddress,
    pub ipv4_addr: Ipv4Addr,
    ifindex: i32,
    socket: Rc<RefCell<RawSocket>>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Linux Runtime
impl LinuxRuntime {
    /// Instantiates a Linux Runtime.
    pub fn new(link_addr: MacAddress, ipv4_addr: Ipv4Addr, ifname: &str, arp: HashMap<Ipv4Addr, MacAddress>) -> Self {
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
            tcp_options: TcpConfig::default(),
            udp_options: UdpConfig::default(),
            arp_options,
            link_addr,
            ipv4_addr,
            ifindex,
            socket: Rc::new(RefCell::new(socket)),
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

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for LinuxRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for LinuxRuntime {}
