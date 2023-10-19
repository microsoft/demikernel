// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod engine;
pub mod runtime;

pub use self::{
    engine::SharedEngine,
    runtime::SharedTestRuntime,
};
use crate::runtime::network::{
    config::{
        ArpConfig,
        TcpConfig,
        UdpConfig,
    },
    types::MacAddress,
};
use ::std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Constants
//==============================================================================

pub const RECEIVE_WINDOW_SIZE: usize = 1024;
pub const ALICE_MAC: MacAddress = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);
pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
pub const BOB_MAC: MacAddress = MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);
pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const CARRIE_MAC: MacAddress = MacAddress::new([0xef, 0xcd, 0xab, 0x89, 0x67, 0x45]);
pub const CARRIE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 3);

//==============================================================================
// Standalone Functions
//==============================================================================

pub fn new_alice<const N: usize>(now: Instant) -> SharedEngine<N> {
    let arp_config = ArpConfig::new(
        Some(Duration::from_secs(600)),
        Some(Duration::from_secs(1)),
        Some(2),
        Some(HashMap::new()),
        Some(false),
    );
    let udp_config: UdpConfig = UdpConfig::default();
    let tcp_config: TcpConfig = TcpConfig::default();
    let test_rig: SharedTestRuntime =
        SharedTestRuntime::new(now, arp_config, udp_config, tcp_config, ALICE_MAC, ALICE_IPV4);
    SharedEngine::new(test_rig).unwrap()
}

pub fn new_bob<const N: usize>(now: Instant) -> SharedEngine<N> {
    let arp_config = ArpConfig::new(
        Some(Duration::from_secs(600)),
        Some(Duration::from_secs(1)),
        Some(2),
        Some(HashMap::new()),
        Some(false),
    );
    let udp_config = UdpConfig::default();
    let tcp_config = TcpConfig::default();
    let test_rig = SharedTestRuntime::new(now, arp_config, udp_config, tcp_config, BOB_MAC, BOB_IPV4);
    SharedEngine::new(test_rig).unwrap()
}

pub fn new_alice2<const N: usize>(now: Instant) -> SharedEngine<N> {
    let mut arp: HashMap<Ipv4Addr, MacAddress> = HashMap::<Ipv4Addr, MacAddress>::new();
    arp.insert(ALICE_IPV4, ALICE_MAC);
    arp.insert(BOB_IPV4, BOB_MAC);
    let arp_config = ArpConfig::new(
        Some(Duration::from_secs(600)),
        Some(Duration::from_secs(1)),
        Some(2),
        Some(arp),
        Some(false),
    );
    let udp_config = UdpConfig::default();
    let tcp_config = TcpConfig::default();
    let test_rig = SharedTestRuntime::new(now, arp_config, udp_config, tcp_config, ALICE_MAC, ALICE_IPV4);
    SharedEngine::new(test_rig).unwrap()
}

pub fn new_bob2<const N: usize>(now: Instant) -> SharedEngine<N> {
    let mut arp: HashMap<Ipv4Addr, MacAddress> = HashMap::<Ipv4Addr, MacAddress>::new();
    arp.insert(BOB_IPV4, BOB_MAC);
    arp.insert(ALICE_IPV4, ALICE_MAC);
    let arp_config = ArpConfig::new(
        Some(Duration::from_secs(600)),
        Some(Duration::from_secs(1)),
        Some(2),
        Some(arp),
        Some(false),
    );
    let udp_config = UdpConfig::default();
    let tcp_config = TcpConfig::default();
    let test_rig = SharedTestRuntime::new(now, arp_config, udp_config, tcp_config, BOB_MAC, BOB_IPV4);
    SharedEngine::new(test_rig).unwrap()
}

pub fn new_carrie<const N: usize>(now: Instant) -> SharedEngine<N> {
    let arp_config = ArpConfig::new(
        Some(Duration::from_secs(600)),
        Some(Duration::from_secs(1)),
        Some(2),
        Some(HashMap::new()),
        Some(false),
    );
    let udp_config = UdpConfig::default();
    let tcp_config = TcpConfig::default();

    let test_rig = SharedTestRuntime::new(now, arp_config, udp_config, tcp_config, CARRIE_MAC, CARRIE_IPV4);
    SharedEngine::new(test_rig).unwrap()
}
