use super::{cache::ArpCacheOptions, options::ArpOptions};
use crate::{
    prelude::*, protocols::ethernet2::MacAddress, rand::Seed, Options,
};
use serde_yaml;
use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

lazy_static! {
    static ref TTL: Duration = Duration::new(1, 0);
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

fn new_station<'a>(
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    now: Instant,
) -> Station<'a> {
    Station::from_options(
        now,
        Options {
            my_link_addr: link_addr,
            my_ipv4_addr: ipv4_addr,
            rng_seed: Seed::default(),
            arp: ArpOptions {
                cache: ArpCacheOptions {
                    default_ttl: Some(*TTL),
                },
            },
        },
    )
}

fn new_alice<'a>(now: Instant) -> Station<'a> {
    new_station(*ALICE_MAC, *ALICE_IPV4, now)
}

fn new_bob<'a>(now: Instant) -> Station<'a> {
    new_station(*BOB_MAC, *BOB_IPV4, now)
}

fn new_carrie<'a>(now: Instant) -> Station<'a> {
    new_station(*CARRIE_MAC, *CARRIE_IPV4, now)
}

#[test]
fn basic() {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = new_alice(now);
    let mut bob = new_bob(now);
    let mut carrie = new_carrie(now);

    let mut fut = alice.arp_query(*CARRIE_IPV4);
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        x => panic!("expected Fail::TryAgain {{}}, got `{:?}`", x),
    }

    let request = {
        let effect = alice.pop_effect().expect("expected an effect");
        match effect {
            Effect::Transmit(packet) => packet,
        }
    };

    match bob.receive(request.clone()) {
        Err(Fail::Ignored {}) => (),
        x => panic!("expected Fail::Ignored {{}}, got `{:?}`", x),
    }

    carrie.receive(request).unwrap();
    let reply = {
        let effect = carrie.pop_effect().expect("expected an effect");
        match effect {
            Effect::Transmit(packet) => packet,
        }
    };

    alice.receive(reply).unwrap();
    eprintln!(
        "# ARP cache: \n{}",
        serde_yaml::to_string(&alice.export_arp_cache()).unwrap()
    );
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Ok(link_addr) => assert_eq!(*CARRIE_MAC, link_addr),
        x => panic!("expected future completion, got `{:?}`", x),
    }
}
