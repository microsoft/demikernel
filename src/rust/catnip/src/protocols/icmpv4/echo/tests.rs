use super::*;
use crate::test;
use std::time::{Duration, Instant};

#[test]
fn serialization() {
    // ensures that a ICMPv4 Echo datagram serializes correctly.
    trace!("serialization()");
    let mut bytes = Icmpv4Echo::new_vec();
    let mut echo = Icmpv4EchoMut::attach(&mut bytes);
    echo.r#type(Icmpv4EchoOp::Reply);
    echo.id(0xab);
    echo.seq_num(0xcd);
    let echo = echo.unmut();
    assert_eq!(Icmpv4EchoOp::Reply, echo.op());
    assert_eq!(0xab, echo.id());
    assert_eq!(0xcd, echo.seq_num());
}

#[test]
fn ping() {
    // ensures that a ICMPv4 ping exchange succeeds.

    let t0 = Instant::now();
    let now = t0;
    let timeout = Duration::from_secs(1);
    let mut alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(hashmap! {
        *test::alice_ipv4_addr() => *test::alice_link_addr(),
    });

    let fut = alice.ping(*test::bob_ipv4_addr(), Some(timeout));
    assert!(fut.poll(now).is_none());

    let ping_request = {
        let event = alice.poll(now).unwrap().unwrap();
        let bytes = match event {
            Event::Transmit(bytes) => bytes.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let echo = Icmpv4Echo::attach(&bytes).unwrap();
        assert_eq!(echo.op(), Icmpv4EchoOp::Request);
        bytes
    };

    info!("passing ICMPv4 ping request to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&ping_request).unwrap();
    let ping_reply = {
        let event = bob.poll(now).unwrap().unwrap();
        let bytes = match event {
            Event::Transmit(bytes) => bytes.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let echo = Icmpv4Echo::attach(&bytes).unwrap();
        assert_eq!(echo.op(), Icmpv4EchoOp::Reply);
        bytes
    };

    info!("passing ICMPv4 ping reply back to alice...");
    let now = now + Duration::from_micros(1);
    alice.receive(&ping_reply).unwrap();
    let _ = fut.poll(now).unwrap().unwrap();
}

#[test]
fn timeout() {
    // ensures that a ICMPv4 ping exchange succeeds.

    let mut now = Instant::now();
    let timeout = Duration::from_secs(1);
    let alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let fut = alice.ping(*test::bob_ipv4_addr(), Some(timeout));
    match alice.poll(now).unwrap().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let echo = Icmpv4Echo::attach(bytes.as_slice()).unwrap();
            assert_eq!(echo.op(), Icmpv4EchoOp::Request);
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    now += timeout;
    assert!(fut.poll(now).is_none());

    now += Duration::from_micros(1);
    match fut.poll(now).unwrap() {
        Err(Fail::Timeout {}) => (),
        x => panic!("expected `Fail::Timeout`, got `{:?}`", x),
    }
}
