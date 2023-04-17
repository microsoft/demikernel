// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::inetstack::test_helpers;
use ::anyhow::Result;
use ::futures::task::{
    noop_waker_ref,
    Context,
};
use ::std::{
    future::Future,
    pin::Pin,
    task::Poll,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// IPv4 Ping
//==============================================================================

#[test]
fn ipv4_ping() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    let mut alice = test_helpers::new_alice2(now);

    let mut bob = test_helpers::new_bob2(now);

    // Alice pings Bob.
    let mut ping_fut = Box::pin(alice.ipv4_ping(test_helpers::BOB_IPV4, None));
    match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
        Poll::Pending => {},
        _ => anyhow::bail!("Ping should not complete"),
    };

    now += Duration::from_secs(1);
    alice.clock.advance_clock(now);
    bob.clock.advance_clock(now);

    // Bob receives ping request from Alice.
    bob.receive(alice.rt.pop_frame())?;

    // Bob replies to Alice.
    bob.rt.poll_scheduler();

    now += Duration::from_secs(1);
    alice.clock.advance_clock(now);
    bob.clock.advance_clock(now);

    // Alice receives reply from Bob.
    alice.receive(bob.rt.pop_frame())?;
    alice.rt.poll_scheduler();
    let latency: Duration = match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
        Poll::Ready(Ok(latency)) => latency,
        _ => anyhow::bail!("Ping should have completed"),
    };
    crate::ensure_eq!(latency, Duration::from_secs(2));

    Ok(())
}

#[test]
fn ipv4_ping_loop() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    let mut alice = test_helpers::new_alice2(now);

    let mut bob = test_helpers::new_bob2(now);

    for _ in 1..1000 {
        // Alice pings Bob.
        let mut ping_fut = Box::pin(alice.ipv4_ping(test_helpers::BOB_IPV4, None));
        match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
            Poll::Pending => {},
            _ => anyhow::bail!("Ping should not have completed"),
        };

        now += Duration::from_secs(1);
        alice.clock.advance_clock(now);
        bob.clock.advance_clock(now);

        // Bob receives ping request from Alice.
        bob.receive(alice.rt.pop_frame()).unwrap();

        // Bob replies to Alice.
        bob.rt.poll_scheduler();

        now += Duration::from_secs(1);
        alice.clock.advance_clock(now);
        bob.clock.advance_clock(now);

        // Alice receives reply from Bob.
        alice.receive(bob.rt.pop_frame()).unwrap();
        alice.rt.poll_scheduler();
        let latency: Duration = match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
            Poll::Ready(Ok(latency)) => latency,
            _ => anyhow::bail!("Ping should have completed"),
        };
        crate::ensure_eq!(latency, Duration::from_secs(2));
    }

    Ok(())
}
