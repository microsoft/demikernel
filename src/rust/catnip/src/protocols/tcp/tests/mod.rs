use crate::{
    protocols::{
        ip,
        ipv4,
    },
    runtime::Runtime,
    sync::BytesMut,
    test_helpers,
};
use futures::task::noop_waker_ref;
use must_let::must_let;
use std::{
    convert::TryFrom,
    future::Future,
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

#[test]
fn test_connect() {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

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

    // Send data from Alice to Bob
    let buf = BytesMut::from(&vec![0x5a; 32][..]).freeze();
    let mut write_future = alice.tcp_push(alice_fd, buf.clone());
    must_let!(let Poll::Ready(Ok(())) = Future::poll(Pin::new(&mut write_future), &mut ctx));
    alice.rt().poll_scheduler();

    // Receive it on Bob's side.
    bob.receive(alice.rt().pop_frame()).unwrap();
    let mut pop_future = bob.tcp_pop(bob_fd);
    must_let!(let Poll::Ready(Ok(received_buf)) = Future::poll(Pin::new(&mut pop_future), &mut ctx));
    assert_eq!(received_buf, buf);

    // Test closing the socket from the client
    alice.close(alice_fd).unwrap();

    alice.rt().poll_scheduler();
    bob.receive(alice.rt().pop_frame()).unwrap();

    // We need Bob to send a pure ACK before Alice's FIN gets ack'd.
    bob.rt().poll_scheduler();
    now += Duration::from_secs(5);
    bob.rt().advance_clock(now);
    bob.rt().poll_scheduler();

    alice.receive(bob.rt().pop_frame()).unwrap();
    alice.receive(bob.rt().pop_frame()).unwrap();
    alice.rt().poll_scheduler();

    bob.close(bob_fd).unwrap();
    bob.rt().poll_scheduler();
    alice.receive(bob.rt().pop_frame()).unwrap();
    alice.rt().poll_scheduler();

    bob.receive(alice.rt().pop_frame()).unwrap();
    bob.rt().poll_scheduler();
}
