use bytes::{
    Bytes,
    BytesMut,
};
use catnip::{
    protocols::{
        ip,
        ipv4,
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
    time::Instant,
};

pub fn one_send_recv_round(
    buf: Bytes,
    now: Instant,
    alice: &mut test_helpers::TestEngine,
    alice_fd: u16,
    bob: &mut test_helpers::TestEngine,
    bob_fd: u16,
) {
    // Send data from Alice to Bob
    alice.tcp_write(alice_fd, buf.clone()).unwrap();
    alice.advance_clock(now);
    bob.receive(&alice.rt().pop_frame()).unwrap();

    // Receive it on Bob's side.
    must_let!(let Ok(received_buf) = bob.tcp_read(bob_fd));
    assert_eq!(received_buf.len(), buf.len());

    // Send data from Bob to Alice
    bob.tcp_write(bob_fd, buf.clone()).unwrap();
    bob.advance_clock(now);
    alice.receive(&bob.rt().pop_frame()).unwrap();

    // Receive it on Alice's side.
    must_let!(let Ok(received_buf) = alice.tcp_read(alice_fd));
    assert_eq!(received_buf.len(), buf.len());
}

#[test]
fn send_recv_loop() {
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

    let size = 32;
    let buf = BytesMut::from(&vec![0u8; size][..]).freeze();

    // Send data from Alice to Bob
    alice.tcp_write(alice_fd, buf.clone()).unwrap();
    alice.advance_clock(now);
    bob.receive(&alice.rt().pop_frame()).unwrap();

    // Receive it on Bob's side.
    must_let!(let Ok(buf) = bob.tcp_read(bob_fd));
    assert_eq!(buf.len(), size);

    let num_rounds: usize = env::var("SEND_RECV_ITERS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1);
    for _ in 0..num_rounds {
        one_send_recv_round(buf.clone(), now, &mut alice, alice_fd, &mut bob, bob_fd);
    }
}
