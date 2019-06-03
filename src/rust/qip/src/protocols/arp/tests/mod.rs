use crate::{prelude::*, test};
use serde_yaml;
use std::time::{Duration, Instant};

#[test]
fn immediate_reply() {
    // tests to ensure that an are request results in a reply.
    let now = Instant::now();
    let mut alice = test::new_alice(now);
    let mut bob = test::new_bob(now);
    let mut carrie = test::new_carrie(now);

    let fut = alice.arp_query(*test::CARRIE_IPV4);
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Err(Fail::TryAgain {}) => (),
        x => panic!("expected Fail::TryAgain {{}}, got `{:?}`", x),
    }

    let request = {
        let effect = alice.poll(now).expect("expected an effect");
        match effect {
            Effect::Transmit(packet) => packet,
        }
    };

    match bob.receive(request.clone()) {
        Err(Fail::Ignored {}) => (),
        x => panic!("expected Fail::Ignored {{}}, got `{:?}`", x),
    }

    carrie.receive(request).unwrap();
    let reply = {
        let effect = carrie.poll(now).expect("expected an effect");
        match effect {
            Effect::Transmit(packet) => packet,
        }
    };

    alice.receive(reply).unwrap();
    eprintln!(
        "# ARP cache: \n{}",
        serde_yaml::to_string(&alice.export_arp_cache()).unwrap()
    );
    let now = now + Duration::from_millis(1);
    match fut.poll(now) {
        Ok(link_addr) => assert_eq!(*test::CARRIE_MAC, link_addr),
        x => panic!("expected future completion, got `{:?}`", x),
    }
}
