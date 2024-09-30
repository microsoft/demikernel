// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod libos;
pub mod runtime;

use ::std::net::{IpAddr, Ipv4Addr};

pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
pub const ALICE_IP: IpAddr = IpAddr::V4(ALICE_IPV4);

pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const BOB_IP: IpAddr = IpAddr::V4(BOB_IPV4);

pub const ALICE_CONFIG_PATH: &str = "./tests/rust/common/alice.yaml";
pub const BOB_CONFIG_PATH: &str = "./tests/rust/common/bob.yaml";

pub const PORT_NUMBER: u16 = 1234;
