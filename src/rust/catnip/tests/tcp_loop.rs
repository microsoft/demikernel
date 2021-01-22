#![feature(const_fn, const_mut_refs, const_type_name)]

use catnip::{
    file_table::FileDescriptor,
    protocols::{
        ip,
        ipv4,
    },
    sync::{
        Bytes,
        BytesMut,
    },
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

#[macro_use]
extern crate log;

pub fn one_send_recv_round(
    ctx: &mut Context,
    buf: Bytes,
    alice: &mut test_helpers::TestEngine,
    alice_fd: FileDescriptor,
    bob: &mut test_helpers::TestEngine,
    bob_fd: FileDescriptor,
) {
    let _s = static_span!("tcp_round");

    // Send data from Alice to Bob
    debug!("Sending from Alice to Bob");
    let mut push_future = alice.tcp_push(alice_fd, buf.clone());
    must_let!(let Poll::Ready(Ok(())) = Future::poll(Pin::new(&mut push_future), ctx));
    alice.rt().poll_scheduler();
    bob.receive(alice.rt().pop_frame()).unwrap();

    // Receive it on Bob's side.
    let mut pop_future = bob.tcp_pop(bob_fd);
    must_let!(let Poll::Ready(Ok(received_buf)) = Future::poll(Pin::new(&mut pop_future), ctx));
    debug_assert_eq!(received_buf, buf);

    // Send data from Bob to Alice
    debug!("Sending from Bob to Alice");
    let mut push_future = bob.tcp_push(bob_fd, buf.clone());
    must_let!(let Poll::Ready(Ok(())) = Future::poll(Pin::new(&mut push_future), ctx));
    bob.rt().poll_scheduler();
    alice.receive(bob.rt().pop_frame()).unwrap();

    // Receive it on Alice's side.
    let mut pop_future = alice.tcp_pop(alice_fd);
    must_let!(let Poll::Ready(Ok(received_buf)) = Future::poll(Pin::new(&mut pop_future), ctx));
    debug_assert_eq!(received_buf, buf);
}

#[test]
fn tcp_loop() {
    catnip::logging::initialize();

    let mut ctx = Context::from_waker(noop_waker_ref());
    let now = Instant::now();
    let mut alice = test_helpers::new_alice(now);
    let mut bob = test_helpers::new_bob(now);

    // Establish the connection between the two peers.
    let listen_port = ip::Port::try_from(80).unwrap();
    let listen_addr = ipv4::Endpoint::new(test_helpers::BOB_IPV4, listen_port);

    let listen_fd = bob.tcp_socket();
    bob.tcp_bind(listen_fd, listen_addr).unwrap();
    bob.tcp_listen(listen_fd, 1).unwrap();
    let mut accept_future = bob.tcp_accept(listen_fd);

    let alice_fd = alice.tcp_socket();
    let mut connect_future = alice.tcp_connect(alice_fd, listen_addr);

    // Send the SYN from Alice to Bob
    alice.rt().poll_scheduler();
    bob.receive(alice.rt().pop_frame()).unwrap();

    // Send the SYN+ACK from Bob to Alice
    bob.rt().poll_scheduler();
    alice.receive(bob.rt().pop_frame()).unwrap();

    // Send the ACK from Alice to Bob
    alice.rt().poll_scheduler();
    bob.receive(alice.rt().pop_frame()).unwrap();

    must_let!(let Poll::Ready(Ok(bob_fd)) = Future::poll(Pin::new(&mut accept_future), &mut ctx));
    must_let!(let Poll::Ready(Ok(())) = Future::poll(Pin::new(&mut connect_future), &mut ctx));

    let size = 2048;
    let mut buf = BytesMut::zeroed(size);
    for i in 0..size {
        buf[i] = i as u8;
    }
    let buf = buf.freeze();

    let num_rounds: usize = env::var("SEND_RECV_ITERS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1);

    let mut samples = Vec::with_capacity(num_rounds);

    for _ in 0..num_rounds {
        let start = Instant::now();
        one_send_recv_round(
            &mut ctx,
            buf.clone(),
            &mut alice,
            alice_fd,
            &mut bob,
            bob_fd,
        );
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
