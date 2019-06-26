use super::*;
use crate::test;
use std::time::{Duration, Instant};

#[test]
fn serialization() {
    // ensures that a ICMPv4 Echo datagram serializes correctly.
    trace!("serialization()");
    let mut bytes = Icmpv4EchoMut::new_bytes();
    let mut echo = Icmpv4EchoMut::from_bytes(&mut bytes).unwrap();
    echo.r#type(Icmpv4EchoOp::Reply);
    echo.id(0xab);
    echo.seq_num(0xcd);
    let echo = echo.seal().unwrap();
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
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        x => panic!("expected `Fail::TryAgain`, got `{:?}`", x),
    }

    let ping_request = {
        let effect = alice.poll(now).expect("expected an effect");
        let bytes = match effect {
            Effect::Transmit(bytes) => bytes.to_vec(),
            e => panic!("got unanticipated effect `{:?}`", e),
        };

        let echo = Icmpv4Echo::from_bytes(&bytes).unwrap();
        assert_eq!(echo.op(), Icmpv4EchoOp::Request);
        bytes
    };

    info!("passing ICMPv4 ping request to bob...");
    let now = now + Duration::from_millis(1);
    bob.receive(&ping_request).unwrap();
    let ping_reply = {
        let effect = bob.poll(now).expect("expected an effect");
        let bytes = match effect {
            Effect::Transmit(bytes) => bytes.to_vec(),
            e => panic!("got unanticipated effect `{:?}`", e),
        };

        let echo = Icmpv4Echo::from_bytes(&bytes).unwrap();
        assert_eq!(echo.op(), Icmpv4EchoOp::Reply);
        bytes
    };

    info!("passing ICMPv4 ping reply back to alice...");
    let now = now + Duration::from_millis(1);
    alice.receive(&ping_reply).unwrap();
    match fut.poll(now) {
        Ok(dt) => assert_eq!(dt, now - t0),
        x => panic!("expected `Ok(_)`, got `{:?}`", x),
    }
}

#[test]
fn timeout() {
    // ensures that a ICMPv4 ping exchange succeeds.

    let t0 = Instant::now();
    let now = t0;
    let timeout = Duration::from_secs(1);
    let alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let fut = alice.ping(*test::bob_ipv4_addr(), Some(timeout));
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        x => panic!("expected `Fail::TryAgain`, got `{:?}`", x),
    }

    let now = now + timeout;
    match fut.poll(now) {
        Err(Fail::Timeout {}) => (),
        x => panic!("expected `Fail::Timeout`, got `{:?}`", x),
    }
}
