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
use crate::{
    demikernel::config::Config,
    runtime::{
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
    },
};
use ::std::{
    collections::HashMap,
    fs,
    net::Ipv4Addr,
    num::ParseIntError,
    time::Duration,
};

//==============================================================================
// Constants & Structures
//==============================================================================

/// Linux Runtime
#[derive(Clone)]
pub struct LinuxRuntime {
    tcp_config: TcpConfig,
    udp_config: UdpConfig,
    arp_config: ArpConfig,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    ifindex: i32,
    socket: RawSocket,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Linux Runtime
impl LinuxRuntime {
    /// Instantiates a Linux Runtime.
    pub fn new(config: Config) -> Self {
        let arp_config: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(HashMap::<Ipv4Addr, MacAddress>::default()),
            Some(false),
        );

        // TODO: Make this constructor return a Result and drop expect() calls below.
        let mac_addr: [u8; 6] = [0; 6];
        let ifindex: i32 = Self::get_ifindex(&config.local_interface_name()).expect("could not parse ifindex");
        let socket: RawSocket = RawSocket::new().expect("could not create raw socket");
        let sockaddr: RawSocketAddr = RawSocketAddr::new(ifindex, &mac_addr);
        socket.bind(&sockaddr).expect("could not bind raw socket");

        Self {
            tcp_config: TcpConfig::default(),
            udp_config: UdpConfig::default(),
            arp_config,
            link_addr: config.local_link_addr(),
            ipv4_addr: config.local_ipv4_addr(),
            ifindex,
            socket,
        }
    }

    /// Gets the interface index of the network interface named `ifname`.
    fn get_ifindex(ifname: &str) -> Result<i32, ParseIntError> {
        let path: String = format!("/sys/class/net/{}/ifindex", ifname);
        fs::read_to_string(path).expect("could not read ifname").trim().parse()
    }

    pub fn get_link_addr(&self) -> MacAddress {
        self.link_addr
    }

    pub fn get_ip_addr(&self) -> Ipv4Addr {
        self.ipv4_addr
    }

    pub fn get_arp_config(&self) -> ArpConfig {
        self.arp_config.clone()
    }

    pub fn get_udp_config(&self) -> UdpConfig {
        self.udp_config.clone()
    }

    pub fn get_tcp_config(&self) -> TcpConfig {
        self.tcp_config.clone()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Memory Runtime Trait Implementation for POSIX Runtime
impl MemoryRuntime for LinuxRuntime {}

/// Runtime Trait Implementation for POSIX Runtime
impl Runtime for LinuxRuntime {}
