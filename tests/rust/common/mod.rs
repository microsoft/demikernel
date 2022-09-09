// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod libos;
pub mod runtime;

use ::demikernel::runtime::network::types::MacAddress;
use ::std::{
    collections::HashMap,
    net::Ipv4Addr,
};

// Alice Address
pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
pub const ALICE_MAC: MacAddress = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);

// Bob Address
pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const BOB_MAC: MacAddress = MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);

// Port Number used for Tests
pub const PORT_BASE: u16 = 1234;

pub fn arp() -> HashMap<Ipv4Addr, MacAddress> {
    let mut arp: HashMap<Ipv4Addr, MacAddress> = HashMap::default();
    arp.insert(ALICE_IPV4, ALICE_MAC);
    arp.insert(BOB_IPV4, BOB_MAC);
    arp
}
