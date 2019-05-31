use super::{cache::ArpCacheOptions, options::ArpOptions, state::ArpState};
use crate::{prelude::*, rand::Seed, runtime, Options};
use eui48::MacAddress;
use std::{
    cell::RefCell,
    net::Ipv4Addr,
    rc::Rc,
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

fn new_arp<'a>(
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    now: Instant,
) -> ArpState<'a> {
    let rt = Rc::new(RefCell::new(runtime::State::from_options(Options {
        my_link_addr: link_addr,
        my_ipv4_addr: ipv4_addr,
        rng_seed: Seed::default(),
        arp: ArpOptions {
            cache: ArpCacheOptions {
                default_ttl: Some(*TTL),
            },
        },
    })));

    ArpState::new(rt, now)
}

fn new_alice<'a>(now: Instant) -> ArpState<'a> {
    new_arp(*ALICE_MAC, *ALICE_IPV4, now)
}

fn new_bob<'a>(now: Instant) -> ArpState<'a> {
    new_arp(*BOB_MAC, *BOB_IPV4, now)
}

fn new_carrie<'a>(now: Instant) -> ArpState<'a> {
    new_arp(*CARRIE_MAC, *CARRIE_IPV4, now)
}

#[test]
fn basic() {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = new_alice(now);
    let mut bob = new_bob(now);
    let mut carrie = new_carrie(now);

    let mut fut = alice.query(*CARRIE_IPV4);
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        _ => panic!("expected Fail::TryAgain {{}}"),
    }

    let request = {
        let rt = alice.rt();
        let mut rt = rt.borrow_mut();
        assert_eq!(1, rt.effects().len());
        let effect = rt.effects().pop_front().unwrap();
        match effect {
            Effect::Transmit(packet) => packet,
            _ => panic!("unexpected type of effect"),
        }
    };

    bob.receive(request.clone()).unwrap();
    let reply = {
        let rt = bob.rt();
        let mut rt = rt.borrow_mut();
        assert_eq!(1, rt.effects().len());
        let effect = rt.effects().pop_front().unwrap();
        match effect {
            Effect::Transmit(packet) => packet,
            _ => panic!("unexpected type of effect"),
        }
    };

    alice.receive(reply).unwrap();
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        _ => panic!("expected Fail::TryAgain {{}}"),
    }

    carrie.receive(request).unwrap();
    let reply = {
        let rt = carrie.rt();
        let mut rt = rt.borrow_mut();
        assert_eq!(1, rt.effects().len());
        let effect = rt.effects().pop_front().unwrap();
        match effect {
            Effect::Transmit(packet) => packet,
            _ => panic!("unexpected type of effect"),
        }
    };

    alice.receive(reply).unwrap();
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Ok(link_addr) => assert_eq!(*CARRIE_MAC, link_addr),
        _ => panic!("expected future completion"),
    }
}
