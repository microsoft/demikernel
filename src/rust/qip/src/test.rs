use crate::{
    prelude::*,
    protocols::{
        arp::{ArpCacheOptions, ArpOptions},
        ethernet2::MacAddress,
    },
    Options,
};
use flexi_logger::Logger;
use float_duration::FloatDuration;
use std::{
    net::Ipv4Addr,
    sync::{Once, ONCE_INIT},
    time::Instant,
};

lazy_static! {
    static ref DEFAULT_TIMEOUT: FloatDuration = FloatDuration::seconds(1.0);
    static ref DEFAULT_TTL: FloatDuration = FloatDuration::seconds(10.0);
    static ref ALICE_MAC: MacAddress =
        MacAddress::new([0x11, 0x11, 0x11, 0x11, 0x11, 0x11]);
    static ref ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
    static ref BOB_MAC: MacAddress =
        MacAddress::new([0x22, 0x22, 0x22, 0x22, 0x22, 0x22]);
    static ref BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
    static ref CARRIE_MAC: MacAddress =
        MacAddress::new([0x33, 0x33, 0x33, 0x33, 0x33, 0x33]);
    static ref CARRIE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 3);
}

static INIT_LOG: Once = ONCE_INIT;

fn initialize_logger() {
    INIT_LOG.call_once(|| {
        Logger::with_env_or_str("").start().unwrap();
    });
}

pub fn new_station<'a>(
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    now: Instant,
) -> Station<'a> {
    initialize_logger();
    Station::from_options(
        now,
        Options {
            my_link_addr: link_addr,
            my_ipv4_addr: ipv4_addr,
            arp: ArpOptions {
                request_timeout: Some(*DEFAULT_TIMEOUT),
                retry_count: Some(2),
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

pub fn alice_ipv4_addr() -> &'static Ipv4Addr {
    &ALICE_IPV4
}

pub fn alice_link_addr() -> &'static MacAddress {
    &ALICE_MAC
}

pub fn new_bob<'a>(now: Instant) -> Station<'a> {
    new_station(*BOB_MAC, *BOB_IPV4, now)
}

pub fn bob_ipv4_addr() -> &'static Ipv4Addr {
    &BOB_IPV4
}

pub fn bob_link_addr() -> &'static MacAddress {
    &BOB_MAC
}

pub fn new_carrie<'a>(now: Instant) -> Station<'a> {
    new_station(*CARRIE_MAC, *CARRIE_IPV4, now)
}

pub fn carrie_ipv4_addr() -> &'static Ipv4Addr {
    &CARRIE_IPV4
}

pub fn carrie_link_addr() -> &'static MacAddress {
    &CARRIE_MAC
}
