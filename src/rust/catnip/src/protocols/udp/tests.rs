use super::datagram::UdpDatagram;
use crate::{
    prelude::*,
    protocols::{icmpv4, ip, ipv4},
    r#async::Async,
    test,
};
use std::time::{Duration, Instant};

#[test]
fn unicast() {
    // ensures that a UDP cast succeeds.

    let alice_port = ip::Port::try_from(54321).unwrap();
    let bob_port = ip::Port::try_from(12345).unwrap();

    let now = Instant::now();
    let text = vec![0xffu8; 10];
    let alice = test::new_alice(now);
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(hashmap! {
        *test::alice_ipv4_addr() => *test::alice_link_addr(),
    });
    bob.open_udp_port(bob_port);

    let fut = alice.udp_cast(
        *test::bob_ipv4_addr(),
        bob_port,
        alice_port,
        text.clone(),
    );
    let now = now + Duration::from_millis(1);
    fut.poll(now).unwrap().unwrap();

    let udp_datagram = {
        let event = alice.poll(now).unwrap().unwrap();
        let bytes = match event {
            Event::Transmit(datagram) => datagram.to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let _ = UdpDatagram::attach(&bytes).unwrap();
        bytes
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    match event {
        Event::BytesReceived {
            ref protocol,
            ref src_addr,
            ref src_port,
            ref dest_port,
            text: ref p,
        } => {
            assert_eq!(protocol, &ipv4::Protocol::Udp);
            assert_eq!(src_addr, test::alice_ipv4_addr());
            assert_eq!(src_port, &alice_port);
            assert_eq!(dest_port, &bob_port);
            assert_eq!(p.len(), 1);
            assert_eq!(text.as_slice(), &p[0][..text.len()]);
            for i in &p[0][text.len()..] {
                assert_eq!(&0u8, i);
            }
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
    alice.import_arp_cache(hashmap! {
        *test::bob_ipv4_addr() => *test::bob_link_addr(),
    });

    let mut bob = test::new_bob(now);
    bob.import_arp_cache(hashmap! {
        *test::alice_ipv4_addr() => *test::alice_link_addr(),
    });

    let fut = alice.udp_cast(
        *test::bob_ipv4_addr(),
        bob_port,
        alice_port,
        text.clone(),
    );
    let now = now + Duration::from_millis(1);
    fut.poll(now).unwrap().unwrap();

    let udp_datagram = {
        let event = alice.poll(now).unwrap().unwrap();
        let bytes = match event {
            Event::Transmit(datagram) => datagram.to_vec(),
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let _ = UdpDatagram::attach(&bytes).unwrap();
        bytes
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    let event = bob.poll(now).unwrap().unwrap();
    let icmpv4_datagram = {
        let bytes = match event {
            Event::Transmit(bytes) => bytes,
            e => panic!("got unanticipated event `{:?}`", e),
        };

        let _ = icmpv4::Error::attach(&bytes).unwrap();
        bytes
    };

    info!("passing ICMPv4 datagram to alice...");
    alice.receive(&icmpv4_datagram).unwrap();
    let event = alice.poll(now).unwrap().unwrap();
    match event {
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
