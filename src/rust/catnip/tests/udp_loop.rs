#![feature(const_fn, const_mut_refs, const_type_name)]

use catnip::{
    engine::Protocol,
    protocols::{
        ip,
        ipv4,
    },
    sync::BytesMut,
    test_helpers,
};
use futures::{
    task::noop_waker_ref,
    Future,
};
use must_let::must_let;
use std::{
    convert::TryFrom,
    env,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
    time::{
        Duration,
        Instant,
    },
};
use tracy_client::static_span;

#[test]
fn udp_loop() {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let now = Instant::now();
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);

    let port = ip::Port::try_from(80).unwrap();
    let alice_addr = ipv4::Endpoint::new(test_helpers::ALICE_IPV4, port);
    let bob_addr = ipv4::Endpoint::new(test_helpers::BOB_IPV4, port);

    let alice_fd = alice.socket(Protocol::Udp);
    let _ = alice.bind(alice_fd, alice_addr);
    let _ = alice.connect(alice_fd, bob_addr);

    let bob_fd = bob.socket(Protocol::Udp);
    let _ = bob.bind(bob_fd, bob_addr);
    let _ = bob.connect(bob_fd, alice_addr);

    let size = 32;
    let buf = BytesMut::from(&vec![0u8; size][..]).freeze();

    let num_rounds: usize = env::var("SEND_RECV_ITERS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1);

    let mut samples = Vec::with_capacity(num_rounds);

    for _ in 0..num_rounds {
        let _s = static_span!("udp round");
        let start = Instant::now();

        alice.udp_push(alice_fd, buf.clone()).unwrap();
        alice.rt().poll_scheduler();
        bob.receive(alice.rt().pop_frame()).unwrap();

        let mut pop_future = bob.udp_pop(bob_fd);
        must_let!(let Poll::Ready(Ok((_, recv_buf))) = Future::poll(Pin::new(&mut pop_future), &mut ctx));
        assert_eq!(recv_buf.len(), buf.len());

        bob.udp_push(bob_fd, recv_buf).unwrap();
        bob.rt().poll_scheduler();
        alice.receive(bob.rt().pop_frame()).unwrap();

        let mut pop_future = alice.udp_pop(alice_fd);
        must_let!(let Poll::Ready(Ok((_, recv_buf))) = Future::poll(Pin::new(&mut pop_future), &mut ctx));
        assert_eq!(recv_buf.len(), buf.len());

        samples.push(start.elapsed());
    }

    let mut h = histogram::Histogram::new();
    for s in samples {
        h.increment(s.as_nanos() as u64).unwrap();
    }
    println!("Min:   {:?}", Duration::from_nanos(h.minimum().unwrap()));
    println!(
        "p25:   {:?}",
        Duration::from_nanos(h.percentile(0.25).unwrap())
    );
    println!(
        "p50:   {:?}",
        Duration::from_nanos(h.percentile(0.50).unwrap())
    );
    println!(
        "p75:   {:?}",
        Duration::from_nanos(h.percentile(0.75).unwrap())
    );
    println!(
        "p90:   {:?}",
        Duration::from_nanos(h.percentile(0.90).unwrap())
    );
    println!(
        "p95:   {:?}",
        Duration::from_nanos(h.percentile(0.95).unwrap())
    );
    println!(
        "p99:   {:?}",
        Duration::from_nanos(h.percentile(0.99).unwrap())
    );
    println!(
        "p99.9: {:?}",
        Duration::from_nanos(h.percentile(0.999).unwrap())
    );
    println!("Max:   {:?}", Duration::from_nanos(h.maximum().unwrap()));
}
