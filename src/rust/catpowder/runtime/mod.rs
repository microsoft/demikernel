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
    expect_ok,
    runtime::{
        fail::Fail,
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
        SharedObject,
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
    socket: SharedObject<RawSocket>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Linux Runtime
impl LinuxRuntime {
    /// Instantiates a Linux Runtime.
    pub fn new(config: Config) -> Result<Self, Fail> {
        let arp_config: ArpConfig = ArpConfig::new(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(1)),
            Some(2),
            Some(HashMap::<Ipv4Addr, MacAddress>::default()),
        );

        // TODO: Make this constructor return a Result and drop expect() calls below.
        let mac_addr: [u8; 6] = [0; 6];
        let ifindex: i32 = expect_ok!(
            Self::get_ifindex(&config.local_interface_name()?),
            "could not parse ifindex"
        );
        let socket: RawSocket = expect_ok!(RawSocket::new(), "could not create raw socket");
        let sockaddr: RawSocketAddr = RawSocketAddr::new(ifindex, &mac_addr);
        expect_ok!(socket.bind(&sockaddr), "could not bind raw socket");

        Ok(Self {
            tcp_config: TcpConfig::default(),
            udp_config: UdpConfig::default(),
            arp_config,
            link_addr: config.local_link_addr()?,
            ipv4_addr: config.local_ipv4_addr()?,
            ifindex,
            socket: SharedObject::<RawSocket>::new(socket),
        })
    }

    /// Gets the interface index of the network interface named `ifname`.
    fn get_ifindex(ifname: &str) -> Result<i32, ParseIntError> {
        let path: String = format!("/sys/class/net/{}/ifindex", ifname);
        expect_ok!(fs::read_to_string(path), "could not read ifname")
            .trim()
            .parse()
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
