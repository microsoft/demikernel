use crate::{
    prelude::*,
    protocols::{arp, ethernet2::MacAddress, tcp},
    rand::Seed,
    Options,
};
use base64::{encode_config, STANDARD_NO_PAD};
use flexi_logger::Logger;
use float_duration::FloatDuration;
use std::{net::Ipv4Addr, sync::Once, time::Instant};

const RECEIVE_WINDOW_SIZE: usize = 1024;

lazy_static! {
    static ref ALICE_MAC: MacAddress =
        MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);
    static ref ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
    static ref BOB_MAC: MacAddress =
        MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);
    static ref BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
    static ref CARRIE_MAC: MacAddress =
        MacAddress::new([0xef, 0xcd, 0xab, 0x89, 0x67, 0x45]);
    static ref CARRIE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 3);
}

static INIT_LOG: Once = Once::new();

fn initialize_logger() {
    INIT_LOG.call_once(|| {
        Logger::with_env_or_str("").start().unwrap();
    });
}

pub fn new_engine<'a>(
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    now: Instant,
) -> Engine<'a> {
    initialize_logger();
    // we always want to use the same seed for our unit tests.
    let mut seed = Seed::default();
    seed[0..6].copy_from_slice(&link_addr.to_array());
    let seed = encode_config(seed.as_ref(), STANDARD_NO_PAD);
    Engine::from_options(
        now,
        Options {
            my_link_addr: link_addr,
            my_ipv4_addr: ipv4_addr,
            arp: arp::Options {
                request_timeout: Some(FloatDuration::seconds(1.0)),
                retry_count: Some(2),
                cache_ttl: Some(FloatDuration::minutes(5.0)),
            },
            rng_seed: Some(seed),
            tcp: tcp::Options {
                advertised_mss: Some(tcp::MIN_MSS),
                handshake_retries: None,
                handshake_timeout: None,
                receive_window_size: Some(RECEIVE_WINDOW_SIZE),
                retries2: None,
                trailing_ack_delay: None,
            },
        },
    )
    .unwrap()
}

pub fn new_alice<'a>(now: Instant) -> Engine<'a> {
    new_engine(*ALICE_MAC, *ALICE_IPV4, now)
}

pub fn alice_ipv4_addr() -> &'static Ipv4Addr {
    &ALICE_IPV4
}

pub fn alice_link_addr() -> &'static MacAddress {
    &ALICE_MAC
}

pub fn new_bob<'a>(now: Instant) -> Engine<'a> {
    new_engine(*BOB_MAC, *BOB_IPV4, now)
}

pub fn bob_ipv4_addr() -> &'static Ipv4Addr {
    &BOB_IPV4
}

pub fn bob_link_addr() -> &'static MacAddress {
    &BOB_MAC
}

pub fn new_carrie<'a>(now: Instant) -> Engine<'a> {
    new_engine(*CARRIE_MAC, *CARRIE_IPV4, now)
}

pub fn carrie_ipv4_addr() -> &'static Ipv4Addr {
    &CARRIE_IPV4
}

pub fn carrie_link_addr() -> &'static MacAddress {
    &CARRIE_MAC
}
