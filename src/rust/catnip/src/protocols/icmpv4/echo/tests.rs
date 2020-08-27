// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;
use crate::test;
use std::future::Future;
use std::task::Poll;
use futures::FutureExt;
use futures::task::{Context, noop_waker_ref};
use fxhash::FxHashMap;
use std::{
    iter,
    time::{Duration, Instant},
};

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
    alice.import_arp_cache(
        iter::once((*test::bob_ipv4_addr(), *test::bob_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(
        iter::once((*test::alice_ipv4_addr(), *test::alice_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.ping(*test::bob_ipv4_addr(), Some(timeout)).boxed_local();
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());

    let ping_request = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        let bytes = match &*event {
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
        bob.advance_clock(now);
        let event = bob.pop_event().unwrap();
        let bytes = match &*event {
            Event::Transmit(bytes) => bytes.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let echo = Icmpv4Echo::attach(&bytes).unwrap();
        assert_eq!(echo.op(), Icmpv4EchoOp::Reply);
        bytes
    };

    info!("passing ICMPv4 ping reply back to alice...");
    let now = now + Duration::from_micros(1);
    alice.advance_clock(now);
    alice.receive(&ping_reply).unwrap();

    assert!(Future::poll(fut.as_mut(), &mut ctx).is_ready());
}

#[test]
fn timeout() {
    // ensures that a ICMPv4 ping exchange succeeds.

    let mut now = Instant::now();
    let timeout = Duration::from_secs(1);
    let alice = test::new_alice(now);
    alice.import_arp_cache(
        iter::once((*test::bob_ipv4_addr(), *test::bob_link_addr()))
            .collect::<FxHashMap<_, _>>(),
    );

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.ping(*test::bob_ipv4_addr(), Some(timeout)).boxed_local();
    alice.advance_clock(now);
    match &*alice.pop_event().unwrap() {
        Event::Transmit(bytes) => {
            let bytes = bytes.borrow().to_vec();
            let echo = Icmpv4Echo::attach(bytes.as_slice()).unwrap();
            assert_eq!(echo.op(), Icmpv4EchoOp::Request);
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }

    now += timeout;
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());

    now += Duration::from_micros(1);
    match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Err(Fail::Timeout {})) => (),
        x => panic!("expected `Fail::Timeout`, got `{:?}`", x),
    }
}
