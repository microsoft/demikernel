// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#[cfg(any(
    feature = "catnap-libos",
    feature = "catnip-libos",
    feature = "catpowder-libos",
    feature = "catloop-libos"
))]
pub mod libos;

#[cfg(any(
    feature = "catnap-libos",
    feature = "catnip-libos",
    feature = "catpowder-libos",
    feature = "catloop-libos"
))]
pub mod runtime;

use ::std::net::{
    IpAddr,
    Ipv4Addr,
};

// Alice Address
pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
pub const ALICE_IP: IpAddr = IpAddr::V4(ALICE_IPV4);

// Bob Address
pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const BOB_IP: IpAddr = IpAddr::V4(BOB_IPV4);

// Config paths for Alice and Bob.
pub const ALICE_CONFIG_PATH: &str = "./tests/rust/common/alice.yaml";
pub const BOB_CONFIG_PATH: &str = "./tests/rust/common/bob.yaml";

// Port Number used for Tests
pub const PORT_BASE: u16 = 1234;
