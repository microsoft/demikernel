use crate::{
    prelude::*,
    protocols::{
        arp::{ArpCacheOptions, ArpOptions},
        ethernet2::MacAddress,
    },
    Options,
};
use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

lazy_static! {
    pub static ref DEFAULT_TTL: Duration = Duration::new(1, 0);
    pub static ref ALICE_MAC: MacAddress =
        MacAddress::new([0x11, 0x11, 0x11, 0x11, 0x11, 0x11]);
    pub static ref ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
    pub static ref BOB_MAC: MacAddress =
        MacAddress::new([0x22, 0x22, 0x22, 0x22, 0x22, 0x22]);
    pub static ref BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
    pub static ref CARRIE_MAC: MacAddress =
        MacAddress::new([0x33, 0x33, 0x33, 0x33, 0x33, 0x33]);
    pub static ref CARRIE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 3);
}

pub fn new_station<'a>(
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    now: Instant,
) -> Station<'a> {
    Station::from_options(
        now,
        Options {
            my_link_addr: link_addr,
            my_ipv4_addr: ipv4_addr,
            arp: ArpOptions {
                cache: ArpCacheOptions {
                    default_ttl: Some(*DEFAULT_TTL),
                },
            },
        },
    )
}

pub fn new_alice<'a>(now: Instant) -> Station<'a> {
    new_station(*ALICE_MAC, *ALICE_IPV4, now)
}

pub fn new_bob<'a>(now: Instant) -> Station<'a> {
    new_station(*BOB_MAC, *BOB_IPV4, now)
}

pub fn new_carrie<'a>(now: Instant) -> Station<'a> {
    new_station(*CARRIE_MAC, *CARRIE_IPV4, now)
}
