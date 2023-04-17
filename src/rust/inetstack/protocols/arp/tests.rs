// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::packet::{
    ArpHeader,
    ArpOperation,
};
use crate::{
    inetstack::{
        protocols::ethernet2::Ethernet2Header,
        test_helpers::{self,},
    },
    runtime::network::types::MacAddress,
};
use ::anyhow::Result;
use ::futures::{
    task::{
        noop_waker_ref,
        Context,
    },
    FutureExt,
};
use ::libc::{
    EBADMSG,
    ETIMEDOUT,
};
use ::std::{
    future::Future,
    task::Poll,
    time::{
        Duration,
        Instant,
    },
};

/// Tests that requests get replied.
#[test]
fn immediate_reply() -> Result<()> {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);
    let mut carrie = test_helpers::new_carrie(now);

    crate::ensure_eq!(alice.rt.arp_options.get_request_timeout(), Duration::from_secs(1));

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.arp_query(test_helpers::CARRIE_IPV4).boxed_local();
    let now = now + Duration::from_micros(1);
    crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);

    alice.clock.advance_clock(now);
    let request = alice.rt.pop_frame();

    // bob hasn't heard of alice before, so he will ignore the request.
    match bob.receive(request.clone()) {
        Err(e) if e.errno == EBADMSG => {},
        _ => anyhow::bail!("passing ARP request to bob should be ignored"),
    };

    let cache = bob.export_arp_cache();
    crate::ensure_eq!(cache.get(&test_helpers::ALICE_IPV4), None);

    if let Err(e) = carrie.receive(request) {
        anyhow::bail!("receive returned error: {:?}", e);
    }
    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    crate::ensure_eq!(cache.get(&test_helpers::ALICE_IPV4), Some(&test_helpers::ALICE_MAC));

    carrie.clock.advance_clock(now);
    let reply = carrie.rt.pop_frame();

    info!("passing ARP reply back to alice...");
    if let Err(e) = alice.receive(reply) {
        anyhow::bail!("arp returned error: {:?}", e);
    }
    let now = now + Duration::from_micros(1);
    alice.clock.advance_clock(now);
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
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);
    let mut carrie = test_helpers::new_carrie(now);

    // this test is written based on certain assumptions.
    crate::ensure_eq!(alice.rt.arp_options.get_retry_count() > 0, true);
    crate::ensure_eq!(alice.rt.arp_options.get_request_timeout(), Duration::from_secs(1));

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.arp_query(test_helpers::CARRIE_IPV4).boxed_local();

    // move time forward enough to trigger a timeout.
    now += Duration::from_secs(1);
    alice.clock.advance_clock(now);
    crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);

    let request = alice.rt.pop_frame();

    // bob hasn't heard of alice before, so he will ignore the request.
    match bob.receive(request.clone()) {
        Err(e) if e.errno == EBADMSG => {},
        _ => anyhow::bail!("passing ARP request to bob should be ignored"),
    };

    let cache = bob.export_arp_cache();
    crate::ensure_eq!(cache.get(&test_helpers::ALICE_IPV4), None);

    if let Err(e) = carrie.receive(request) {
        anyhow::bail!("receive returned error: {:?}", e);
    }

    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    crate::ensure_eq!(
        cache.get(&test_helpers::ALICE_IPV4),
        Some(&test_helpers::ALICE_MAC),
        "Not equal: {:?} {:?}",
        cache.get(&test_helpers::ALICE_IPV4),
        Some(&test_helpers::ALICE_MAC)
    );

    carrie.clock.advance_clock(now);
    let reply = carrie.rt.pop_frame();

    if let Err(e) = alice.receive(reply) {
        anyhow::bail!("receive returned error: {:?}", e);
    }

    now += Duration::from_micros(1);
    alice.clock.advance_clock(now);
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
    let alice = test_helpers::new_alice(now);

    crate::ensure_eq!(alice.rt.arp_options.get_retry_count(), 2);
    crate::ensure_eq!(alice.rt.arp_options.get_request_timeout(), Duration::from_secs(1));

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.arp_query(test_helpers::CARRIE_IPV4).boxed_local();
    crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);
    let bytes = alice.rt.pop_frame();

    let payload = match Ethernet2Header::parse(bytes) {
        Ok((_, payload)) => payload,
        Err(e) => anyhow::bail!("Could not parse ethernet header: {:?}", e),
    };
    let arp = match ArpHeader::parse(payload) {
        Ok(arp) => arp,
        Err(e) => anyhow::bail!("Could not parse arp header: {:?}", e),
    };
    crate::ensure_eq!(arp.get_operation(), ArpOperation::Request);

    for i in 0..alice.rt.arp_options.get_retry_count() {
        now += alice.rt.arp_options.get_request_timeout();
        alice.clock.advance_clock(now);
        crate::ensure_eq!(Future::poll(fut.as_mut(), &mut ctx).is_pending(), true);
        info!("no_reply(): retry #{}", i + 1);
        let bytes = alice.rt.pop_frame();
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
    now += alice.rt.arp_options.get_request_timeout();
    alice.clock.advance_clock(now);
    match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Err(error)) if error.errno == ETIMEDOUT => Ok(()),
        _ => anyhow::bail!("poll should have succeeded"),
    }
}
