// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::inetstack::test_helpers::{
    self,
    SharedEngine,
};
use ::anyhow::Result;
use ::futures::{
    pin_mut,
    task::{
        noop_waker_ref,
        Context,
    },
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

    let mut alice: SharedEngine = test_helpers::new_alice2(now);

    let mut bob: SharedEngine = test_helpers::new_bob2(now);

    // Alice pings Bob.
    let mut alice_transport = alice.get_transport();
    let ping_fut = alice_transport.ping(test_helpers::BOB_IPV4, None);
    pin_mut!(ping_fut);
    match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
        Poll::Pending => {},
        _ => anyhow::bail!("Ping should not complete"),
    };

    now += Duration::from_secs(1);
    alice.advance_clock(now);
    bob.advance_clock(now);

    // Bob receives ping request from Alice.
    bob.receive(alice.pop_frame())?;

    // Bob replies to Alice.
    bob.poll();

    now += Duration::from_secs(1);
    alice.advance_clock(now);
    bob.advance_clock(now);

    // Alice receives reply from Bob.
    alice.receive(bob.pop_frame())?;
    alice.poll();
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

    let mut alice: SharedEngine = test_helpers::new_alice2(now);

    let mut bob: SharedEngine = test_helpers::new_bob2(now);

    for _ in 1..1000 {
        // Alice pings Bob.
        let mut alice_transport = alice.get_transport();
        let ping_fut = alice_transport.ping(test_helpers::BOB_IPV4, None);
        pin_mut!(ping_fut);
        match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
            Poll::Pending => {},
            _ => anyhow::bail!("Ping should not have completed"),
        };

        now += Duration::from_secs(1);
        alice.advance_clock(now);
        bob.advance_clock(now);

        // Bob receives ping request from Alice.
        bob.receive(alice.pop_frame()).unwrap();

        // Bob replies to Alice.
        bob.poll();

        now += Duration::from_secs(1);
        alice.advance_clock(now);
        bob.advance_clock(now);

        // Alice receives reply from Bob.
        alice.receive(bob.pop_frame()).unwrap();
        alice.poll();
        let latency: Duration = match Future::poll(Pin::new(&mut ping_fut), &mut ctx) {
            Poll::Ready(Ok(latency)) => latency,
            _ => anyhow::bail!("Ping should have completed"),
        };
        crate::ensure_eq!(latency, Duration::from_secs(2));
    }

    Ok(())
}
