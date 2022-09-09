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
fn immediate_reply() {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);
    let mut carrie = test_helpers::new_carrie(now);

    assert_eq!(alice.rt.arp_options.get_request_timeout(), Duration::from_secs(1));

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.arp_query(test_helpers::CARRIE_IPV4).boxed_local();
    let now = now + Duration::from_micros(1);
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());

    alice.clock.advance_clock(now);
    let request = alice.rt.pop_frame();

    // bob hasn't heard of alice before, so he will ignore the request.
    info!("passing ARP request to bob (should be ignored)...");
    assert_eq!(
        match bob.receive(request.clone()) {
            Err(e) if e.errno == EBADMSG => Ok(()),
            _ => Err(()),
        },
        Ok(())
    );
    let cache = bob.export_arp_cache();
    assert!(cache.get(&test_helpers::ALICE_IPV4).is_none());

    carrie.receive(request).unwrap();
    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    assert_eq!(cache.get(&test_helpers::ALICE_IPV4), Some(&test_helpers::ALICE_MAC));

    carrie.clock.advance_clock(now);
    let reply = carrie.rt.pop_frame();

    info!("passing ARP reply back to alice...");
    alice.receive(reply).unwrap();
    let now = now + Duration::from_micros(1);
    alice.clock.advance_clock(now);
    let link_addr = match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Ok(link_addr)) => Ok(link_addr),
        _ => Err(()),
    }
    .unwrap();
    assert_eq!(test_helpers::CARRIE_MAC, link_addr);
}

#[test]
fn slow_reply() {
    // tests to ensure that an are request results in a reply.
    let mut now = Instant::now();
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);
    let mut carrie = test_helpers::new_carrie(now);

    // this test is written based on certain assumptions.
    assert!(alice.rt.arp_options.get_retry_count() > 0);
    assert_eq!(alice.rt.arp_options.get_request_timeout(), Duration::from_secs(1));

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.arp_query(test_helpers::CARRIE_IPV4).boxed_local();

    // move time forward enough to trigger a timeout.
    now += Duration::from_secs(1);
    alice.clock.advance_clock(now);
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());

    let request = alice.rt.pop_frame();

    // bob hasn't heard of alice before, so he will ignore the request.
    info!("passing ARP request to bob (should be ignored)...");
    assert_eq!(
        match bob.receive(request.clone()) {
            Err(e) if e.errno == EBADMSG => Ok(()),
            _ => Err(()),
        },
        Ok(())
    );

    let cache = bob.export_arp_cache();
    assert!(cache.get(&test_helpers::ALICE_IPV4).is_none());

    carrie.receive(request).unwrap();
    info!("passing ARP request to carrie...");
    let cache = carrie.export_arp_cache();
    assert_eq!(cache.get(&test_helpers::ALICE_IPV4), Some(&test_helpers::ALICE_MAC));

    carrie.clock.advance_clock(now);
    let reply = carrie.rt.pop_frame();

    alice.receive(reply).unwrap();
    now += Duration::from_micros(1);
    alice.clock.advance_clock(now);
    let link_addr: MacAddress = match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Ok(link_addr)) => Ok(link_addr),
        _ => Err(()),
    }
    .unwrap();
    assert_eq!(test_helpers::CARRIE_MAC, link_addr);
}

#[test]
fn no_reply() {
    // tests to ensure that an are request results in a reply.
    let mut now = Instant::now();
    let alice = test_helpers::new_alice(now);

    assert_eq!(alice.rt.arp_options.get_retry_count(), 2);
    assert_eq!(alice.rt.arp_options.get_request_timeout(), Duration::from_secs(1));

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.arp_query(test_helpers::CARRIE_IPV4).boxed_local();
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());
    let bytes = alice.rt.pop_frame();

    let (_, payload) = Ethernet2Header::parse(bytes).unwrap();
    let arp = ArpHeader::parse(payload).unwrap();
    assert_eq!(arp.get_operation(), ArpOperation::Request);

    for i in 0..alice.rt.arp_options.get_retry_count() {
        now += alice.rt.arp_options.get_request_timeout();
        alice.clock.advance_clock(now);
        assert!(Future::poll(fut.as_mut(), &mut ctx).is_pending());
        info!("no_reply(): retry #{}", i + 1);
        let bytes = alice.rt.pop_frame();
        let (_, payload) = Ethernet2Header::parse(bytes).unwrap();
        let arp = ArpHeader::parse(payload).unwrap();
        assert_eq!(arp.get_operation(), ArpOperation::Request);
    }

    // timeout
    now += alice.rt.arp_options.get_request_timeout();
    alice.clock.advance_clock(now);
    match Future::poll(fut.as_mut(), &mut ctx) {
        Poll::Ready(Err(error)) if error.errno == ETIMEDOUT => Ok(()),
        _ => Err(()),
    }
    .unwrap();
}
