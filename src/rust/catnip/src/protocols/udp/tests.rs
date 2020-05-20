// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::datagram::UdpDatagramDecoder;
use crate::{
    prelude::*,
    protocols::{icmpv4, ip},
    test,
};
use fxhash::FxHashMap;
use std::{
    iter,
    time::{Duration, Instant},
};

#[test]
fn unicast() {
    // ensures that a UDP cast succeeds.

    let alice_port = ip::Port::try_from(54321).unwrap();
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let text = vec![0xffu8; 10];
    let alice = test::new_alice(now);
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

    let fut = alice.udp_cast(
        *test::bob_ipv4_addr(),
        bob_port,
        alice_port,
        text.clone(),
    );
    let now = now + Duration::from_micros(1);
    fut.poll(now).unwrap().unwrap();

    let udp_datagram = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        let bytes = match &*event {
            Event::Transmit(datagram) => datagram.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let _ = UdpDatagramDecoder::attach(&bytes).unwrap();
        bytes
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    match &*event {
        Event::UdpDatagramReceived(datagram) => {
            assert_eq!(
                datagram.src_ipv4_addr.unwrap(),
                *test::alice_ipv4_addr()
            );
            assert_eq!(datagram.src_port.unwrap(), alice_port);
            assert_eq!(datagram.dest_port.unwrap(), bob_port);
            assert_eq!(text.as_slice(), &datagram.payload[..text.len()]);
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }
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

    let fut = alice.udp_cast(
        *test::bob_ipv4_addr(),
        bob_port,
        alice_port,
        text.clone(),
    );
    let now = now + Duration::from_micros(1);
    fut.poll(now).unwrap().unwrap();

    let udp_datagram = {
        alice.advance_clock(now);
        let event = alice.pop_event().unwrap();
        let bytes = match &*event {
            Event::Transmit(datagram) => datagram.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let _ = UdpDatagramDecoder::attach(&bytes).unwrap();
        bytes
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    bob.advance_clock(now);
    let event = bob.pop_event().unwrap();
    let icmpv4_datagram = {
        let bytes = match &*event {
            Event::Transmit(bytes) => bytes.borrow().to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let _ = icmpv4::Error::attach(&bytes).unwrap();
        bytes
    };

    info!("passing ICMPv4 datagram to alice...");
    alice.receive(&icmpv4_datagram).unwrap();
    alice.advance_clock(now);
    let event = alice.pop_event().unwrap();
    match &*event {
        Event::Icmpv4Error {
            ref id,
            ref next_hop_mtu,
            ..
        } => {
            assert_eq!(
                id,
                &icmpv4::ErrorId::DestinationUnreachable(
                    icmpv4::DestinationUnreachable::DestinationPortUnreachable
                )
            );
            assert_eq!(next_hop_mtu, &0u16);
            // todo: validate `context`
        }
        e => panic!("got unanticipated event `{:?}`", e),
    }
}
