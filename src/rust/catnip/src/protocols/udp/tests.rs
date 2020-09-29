// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::UdpDatagramDecoder;
use crate::event::Event;
use std::convert::TryFrom;
use std::future::Future;
use futures::FutureExt;
use futures::task::{Context, noop_waker_ref};
use std::task::Poll;
use crate::{
    protocols::{icmpv4, ip},
    test,
};
use fxhash::FxHashMap;
use std::{
    iter,
    time::{Duration, Instant},
};
use must_let::must_let;

#[test]
fn unicast() {
    // ensures that a UDP cast succeeds.

    let alice_port = ip::Port::try_from(54321).unwrap();
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let text = vec![0xffu8; 10];
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
    bob.open_udp_port(bob_port);

    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut fut = alice.udp_cast(
        *test::bob_ipv4_addr(),
        bob_port,
        alice_port,
        text.clone(),
    ).boxed_local();
    let now = now + Duration::from_micros(1);
    must_let!(let Poll::Ready(..) = Future::poll(fut.as_mut(), &mut ctx));

    let udp_datagram = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        must_let!(let Event::Transmit(datagram) = &*event);
        let bytes = datagram.borrow().to_vec();
        let _ = UdpDatagramDecoder::attach(&bytes).unwrap();
        bytes
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    must_let!(let Event::UdpDatagramReceived(datagram) = &*event);
    assert_eq!(
        datagram.src_ipv4_addr.unwrap(),
        *test::alice_ipv4_addr()
    );
    assert_eq!(datagram.src_port.unwrap(), alice_port);
    assert_eq!(datagram.dest_port.unwrap(), bob_port);
    assert_eq!(text.as_slice(), &datagram.payload[..text.len()]);
}

#[test]
fn destination_port_unreachable() {
    // ensures that a UDP cast succeeds.

    let alice_port = ip::Port::try_from(54321).unwrap();
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let text = vec![0xffu8; 10];
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
    let mut fut = alice.udp_cast(*test::bob_ipv4_addr(), bob_port, alice_port, text.clone()).boxed_local();
    assert!(Future::poll(fut.as_mut(), &mut ctx).is_ready());

    let now = now + Duration::from_micros(1);
    bob.advance_clock(now);

    let udp_datagram = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        must_let!(let Event::Transmit(datagram) = &*event);
        let bytes = datagram.borrow().to_vec();
        let _ = UdpDatagramDecoder::attach(&bytes).unwrap();
        bytes
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    let icmpv4_datagram = {
        must_let!(let Event::Transmit(bytes) = &*event);
        let bytes = bytes.borrow().to_vec();
        let _ = icmpv4::Error::attach(&bytes).unwrap();
        bytes
    };

    info!("passing ICMPv4 datagram to alice...");
    alice.receive(&icmpv4_datagram).unwrap();
    alice.advance_clock(now);
    let event = alice.pop_event().unwrap();
    must_let!(let Event::Icmpv4Error { ref id, ref next_hop_mtu, .. } = &*event);
    assert_eq!(
        id,
        &icmpv4::ErrorId::DestinationUnreachable(
            icmpv4::DestinationUnreachable::DestinationPortUnreachable
        )
    );
    assert_eq!(next_hop_mtu, &0u16);
    // todo: validate `context`
}
