// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::*;
use crate::test_helpers;
use futures::{
    task::{
        noop_waker_ref,
        Context,
    },
    FutureExt,
};
use must_let::must_let;
use std::{
    future::Future,
    task::Poll,
    time::{
        Duration,
        Instant,
    },
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
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice
        .ping(test_helpers::BOB_IPV4, Some(timeout))
        .boxed_local();
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());

    let ping_request = {
        alice.advance_clock(now);
        let bytes = alice.rt().pop_frame();
        let echo = Icmpv4Echo::attach(&bytes).unwrap();
        assert_eq!(echo.op(), Icmpv4EchoOp::Request);
        bytes
    };

    info!("passing ICMPv4 ping request to bob...");
    let now = now + Duration::from_micros(1);
    bob.receive(&ping_request).unwrap();
    let ping_reply = {
        bob.advance_clock(now);
        let bytes = bob.rt().pop_frame();
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
    let mut alice = test_helpers::new_alice(now);

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice
        .ping(test_helpers::BOB_IPV4, Some(timeout))
        .boxed_local();

    alice.advance_clock(now);
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());

    let bytes = alice.rt().pop_frame();
    let echo = Icmpv4Echo::attach(bytes.as_slice()).unwrap();
    assert_eq!(echo.op(), Icmpv4EchoOp::Request);

    now += timeout;
    alice.advance_clock(now);
    must_let!(let Poll::Ready(Err(Fail::Timeout {})) = Future::poll(fut.as_mut(), &mut ctx));
}
