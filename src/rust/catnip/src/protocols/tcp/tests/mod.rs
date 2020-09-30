use crate::{
    protocols::{
        ip,
        ipv4,
    },
    test_helpers,
};
use bytes::BytesMut;
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
    time::Instant,
};

#[test]
fn test_connect() {
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
    let mut accept_future = bob.tcp_accept_async(listen_fd);

    let alice_fd = alice.tcp_socket();
    let mut connect_future = alice.tcp_connect(alice_fd, listen_addr);

    // Send the SYN from Alice to Bob
    alice.advance_clock(now);
    bob.receive(&alice.rt().pop_frame()).unwrap();

    // Send the SYN+ACK from Bob to Alice
    bob.advance_clock(now);
    alice.receive(&bob.rt().pop_frame()).unwrap();

    // Send the ACK from Alice to Bob
    alice.advance_clock(now);
    bob.receive(&alice.rt().pop_frame()).unwrap();

    must_let!(let Poll::Ready(Ok(bob_fd)) = Future::poll(Pin::new(&mut accept_future), &mut ctx));
    must_let!(let Poll::Ready(Ok(alice_fd)) = Future::poll(Pin::new(&mut connect_future), &mut ctx));

    // Send data from Alice to Bob
    let buf = BytesMut::from(&vec![0x5a; 32][..]).freeze();
    alice.tcp_write(alice_fd, buf.clone()).unwrap();
    alice.advance_clock(now);

    // Receive it on Bob's side.
    bob.receive(&alice.rt().pop_frame()).unwrap();
    must_let!(let Ok(received_buf) = bob.tcp_read(bob_fd));
    assert_eq!(received_buf, buf);
}
