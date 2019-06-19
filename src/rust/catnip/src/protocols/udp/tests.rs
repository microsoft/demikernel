use crate::{prelude::*, protocols::ipv4, test};
use std::time::{Duration, Instant};

#[test]
fn cast() {
    // ensures that a UDP cast succeeds.

    const ALICE_PORT: u16 = 54321;
    const BOB_PORT: u16 = 12345;

    let now = Instant::now();
    let mut alice = test::new_alice(now);
    let mut bob = test::new_bob(now);
    let payload = vec![0xffu8; 10];

    let fut = alice.udp_cast(
        *test::bob_ipv4_addr(),
        BOB_PORT,
        ALICE_PORT,
        payload.clone(),
    );
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        x => panic!("expected Fail::TryAgain {{}}, got `{:?}`", x),
    }

    let request = {
        let effect = alice.poll(now).expect("expected an effect");
        match effect {
            Effect::Transmit(datagram) => datagram.to_vec(),
            e => panic!("got unanticipated effect `{:?}`", e),
        }
    };

    bob.receive(&request).unwrap();
    info!("passing ARP request to bob...");
    let arp_reply = {
        let effect = bob.poll(now).expect("expected an effect");
        match effect {
            Effect::Transmit(datagram) => datagram.to_vec(),
            e => panic!("got unanticipated effect `{:?}`", e),
        }
    };

    info!("passing ARP reply back to alice...");
    alice.receive(&arp_reply).unwrap();
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        x => {
            debug!("expected future completion (got `{:?}`)", x);
            // the following `panic!()` panics internally, and it's not clear
            // why.
            panic!("expected future completion (got `{:?}`)", x);
        }
    }
    match fut.poll(now) {
        Ok(()) => (),
        x => {
            debug!("expected future completion (got `{:?}`)", x);
            // the following `panic!()` panics internally, and it's not clear
            // why.
            panic!("expected future completion (got `{:?}`)", x);
        }
    }

    let udp_datagram = {
        let effect = alice.poll(now).expect("expected an effect");
        match effect {
            Effect::Transmit(datagram) => datagram.to_vec(),
            e => panic!("got unanticipated effect `{:?}`", e),
        }
    };

    info!("passing UDP datagram to bob...");
    bob.receive(&udp_datagram).unwrap();
    let effect = bob.poll(now).expect("expected an effect");
    match effect {
        Effect::Received {
            ref protocol,
            ref src_addr,
            ref src_port,
            ref dest_port,
            payload: ref p,
        } => {
            assert_eq!(protocol, &ipv4::Protocol::Udp);
            assert_eq!(src_addr, test::alice_ipv4_addr());
            assert_eq!(src_port, &ALICE_PORT);
            assert_eq!(dest_port, &BOB_PORT);
            assert_eq!(p.len(), 1);
            assert_eq!(payload.as_slice(), &p[0][..payload.len()]);
            for i in &p[0][payload.len()..] {
                assert_eq!(&0u8, i);
            }
        }
        e => panic!("got unanticipated effect `{:?}`", e),
    }
}
