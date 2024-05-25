// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod engine;
pub mod runtime;
pub use engine::SharedEngine;
pub use runtime::SharedTestRuntime;

use crate::MacAddress;
use ::std::{
    net::Ipv4Addr,
    time::Instant,
};

//==============================================================================
// Constants
//==============================================================================

pub const ALICE_MAC: MacAddress = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);
pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
pub const BOB_MAC: MacAddress = MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);
pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const CARRIE_MAC: MacAddress = MacAddress::new([0xef, 0xcd, 0xab, 0x89, 0x67, 0x45]);
pub const CARRIE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 3);
pub const ALICE_CONFIG_PATH: &str = "./src/rust/inetstack/test_helpers/alice.yaml";
pub const BOB_CONFIG_PATH: &str = "./src/rust/inetstack/test_helpers/bob.yaml";
pub const CARRIE_CONFIG_PATH: &str = "./src/rust/inetstack/test_helpers/carrie.yaml";

//==============================================================================
// Standalone Functions
//==============================================================================

pub fn new_bob(now: Instant) -> SharedEngine {
    let network: SharedTestRuntime = SharedTestRuntime::new_test(now);
    SharedEngine::new(BOB_CONFIG_PATH, network, now).unwrap()
}

pub fn new_carrie(now: Instant) -> SharedEngine {
    let network = SharedTestRuntime::new_test(now);
    SharedEngine::new(CARRIE_CONFIG_PATH, network, now).unwrap()
}
