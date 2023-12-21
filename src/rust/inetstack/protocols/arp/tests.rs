// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::packet::{
    ArpHeader,
    ArpOperation,
};
use crate::{
    inetstack::{
        protocols::ethernet2::Ethernet2Header,
        test_helpers::{
            self,
            SharedEngine,
        },
    },
    runtime::network::{
        types::MacAddress,
        NetworkRuntime,
    },
};
use ::anyhow::Result;
use ::futures::task::{
    noop_waker_ref,
    Context,
};
use ::libc::ETIMEDOUT;
use ::std::{
    future::Future,
    task::Poll,
    time::{
        Duration,
        Instant,
    },
};
use futures::pin_mut;

/// Tests that requests get replied.
#[test]
fn immediate_reply() -> Result<()> {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice: SharedEngine = test_helpers::new_alice(now);
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let mut alice_transport = alice.get_transport();

    crate::ensure_eq!(
        alice_transport.get_network().get_arp_config().get_request_timeout(),
        Duration::from_secs(1)
    );

    let mut ctx = Context::from_waker(noop_waker_ref());
    let fut = alice_transport.arp_query(test_helpers::CARRIE_IPV4);
    pin_mut!(fut);
    let now = now + Duration::from_micros(1);
    crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);

    alice.advance_clock(now);
    let request = alice.pop_frame();

    // Since receive doesn't return anything, we should get an Ok here.
    // TODO: remove  check for return value once all receives have moved to poliing.
    bob.receive(request.clone())?;
    // check the cache to make sure bob ignored the request.
    let cache = bob.get_transport().export_arp_cache();
    crate::ensure_eq!(cache.get(&test_helpers::ALICE_IPV4), None);

    // Since receive doesn't return anything, we should get an Ok here.
    // TODO: remove  check for return value once all receives have moved to poliing.
    carrie.receive(request)?;
    info!("passing ARP request to carrie...");
    let cache = carrie.get_transport().export_arp_cache();
    crate::ensure_eq!(cache.get(&test_helpers::ALICE_IPV4), Some(&test_helpers::ALICE_MAC));

    carrie.advance_clock(now);
    let reply = carrie.pop_frame();

    info!("passing ARP reply back to alice...");
    if let Err(e) = alice.receive(reply) {
        anyhow::bail!("arp returned error: {:?}", e);
    }
    let now = now + Duration::from_micros(1);
    alice.advance_clock(now);
    let link_addr = match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Ok(link_addr)) => link_addr,
        _ => anyhow::bail!("poll should succeed"),
    };
    crate::ensure_eq!(test_helpers::CARRIE_MAC, link_addr);

    Ok(())
}

#[test]
fn slow_reply() -> Result<()> {
    // tests to ensure that an are request results in a reply.
    let mut now = Instant::now();
    let mut alice: SharedEngine = test_helpers::new_alice(now);
    let mut bob: SharedEngine = test_helpers::new_bob(now);
    let mut carrie: SharedEngine = test_helpers::new_carrie(now);
    let mut alice_transport = alice.get_transport();

    // this test is written based on certain assumptions.
    crate::ensure_eq!(
        alice_transport.get_network().get_arp_config().get_retry_count() > 0,
        true
    );
    crate::ensure_eq!(
        alice_transport.get_network().get_arp_config().get_request_timeout(),
        Duration::from_secs(1)
    );

    let mut ctx = Context::from_waker(noop_waker_ref());
    let fut = alice_transport.arp_query(test_helpers::CARRIE_IPV4);
    pin_mut!(fut);

    // move time forward enough to trigger a timeout.
    now += Duration::from_secs(1);
    alice.advance_clock(now);
    crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);

    let request = alice.pop_frame();

    // bob hasn't heard of alice before, so he will ignore the request.
    // Since receive doesn't return anything, we should get an Ok here.
    // TODO: remove  check for return value once all receives have moved to poliing.
    bob.receive(request.clone())?;
    // check the cache to make sure bob ignored the request.
    let cache = bob.export_arp_cache();
    crate::ensure_eq!(cache.get(&test_helpers::ALICE_IPV4), None);

    // Since receive doesn't return anything, we should get an Ok here.
    // TODO: remove  check for return value once all receives have moved to poliing.
    carrie.receive(request)?;

    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    crate::ensure_eq!(
        cache.get(&test_helpers::ALICE_IPV4),
        Some(&test_helpers::ALICE_MAC),
        "Not equal: {:?} {:?}",
        cache.get(&test_helpers::ALICE_IPV4),
        Some(&test_helpers::ALICE_MAC)
    );

    carrie.advance_clock(now);
    let reply = carrie.pop_frame();

    // Since receive doesn't return anything, we should get an Ok here.
    // TODO: remove  check for return value once all receives have moved to poliing.
    alice.receive(reply)?;

    now += Duration::from_micros(1);
    alice.advance_clock(now);
    let link_addr: MacAddress = match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Ok(link_addr)) => link_addr,
        _ => anyhow::bail!("poll should succeed"),
    };
    crate::ensure_eq!(test_helpers::CARRIE_MAC, link_addr);

    Ok(())
}

#[test]
fn no_reply() -> Result<()> {
    // tests to ensure that an are request results in a reply.
    let mut now = Instant::now();
    let mut alice: SharedEngine = test_helpers::new_alice(now);
    let mut alice_transport = alice.get_transport();

    crate::ensure_eq!(alice_transport.get_network().get_arp_config().get_retry_count(), 2);
    crate::ensure_eq!(
        alice_transport.get_network().get_arp_config().get_request_timeout(),
        Duration::from_secs(1)
    );

    let mut ctx = Context::from_waker(noop_waker_ref());
    let fut = alice_transport.arp_query(test_helpers::CARRIE_IPV4);
    pin_mut!(fut);
    crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);
    let bytes = alice.pop_frame();

    let payload = match Ethernet2Header::parse(bytes) {
        Ok((_, payload)) => payload,
        Err(e) => anyhow::bail!("Could not parse ethernet header: {:?}", e),
    };
    let arp = match ArpHeader::parse(payload) {
        Ok(arp) => arp,
        Err(e) => anyhow::bail!("Could not parse arp header: {:?}", e),
    };
    crate::ensure_eq!(arp.get_operation(), ArpOperation::Request);

    for i in 0..alice.get_transport().get_network().get_arp_config().get_retry_count() {
        now += alice
            .get_transport()
            .get_network()
            .get_arp_config()
            .get_request_timeout();
        alice.advance_clock(now);
        crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);
        info!("no_reply(): retry #{}", i + 1);
        let bytes = alice.pop_frame();
        let payload = match Ethernet2Header::parse(bytes) {
            Ok((_, payload)) => payload,
            Err(e) => anyhow::bail!("Could not parse ethernet header: {:?}", e),
        };
        let arp = match ArpHeader::parse(payload) {
            Ok(arp) => arp,
            Err(e) => anyhow::bail!("Could not parse arp header: {:?}", e),
        };
        crate::ensure_eq!(arp.get_operation(), ArpOperation::Request);
    }

    // timeout
    now += alice
        .get_transport()
        .get_network()
        .get_arp_config()
        .get_request_timeout();
    alice.advance_clock(now);
    match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Err(error)) if error.errno == ETIMEDOUT => Ok(()),
        _ => anyhow::bail!("poll should have succeeded"),
    }
}
