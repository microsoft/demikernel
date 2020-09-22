use bytes::{Bytes, BytesMut};
use std::task::Poll;
use catnip::event::Event;
use must_let::must_let;
use catnip::protocols::ethernet2::MacAddress;
use std::net::Ipv4Addr;
use std::task::Context;
use futures::task::noop_waker_ref;
use catnip::runtime::Runtime;
use catnip::options::Options;
use std::time::Instant;
use catnip::rand::Seed;
use catnip::protocols::{arp, tcp, ip, ipv4};
use catnip::protocols::tcp2::runtime::Runtime as RuntimeTrait;
use futures::Future;
use std::pin::Pin;
use std::convert::TryFrom;
use catnip::protocols::tcp2::peer::Peer;

#[inline(never)]
pub fn send_datagram<RT: RuntimeTrait>(src: &Runtime, dst: &Peer<RT>) {
    must_let!(let Some(event) = src.pop_event());
    must_let!(let Event::Transmit(ref buf) = &*event);
    dst.receive_datagram(ipv4::Datagram::attach(&buf.borrow()).unwrap());
}

#[inline(never)]
pub fn send_datagram1<RT: RuntimeTrait>(src: &Runtime, dst: &Peer<RT>) {
    must_let!(let Some(event) = src.pop_event());
    must_let!(let Event::Transmit(ref buf) = &*event);
    dst.receive_datagram(ipv4::Datagram::attach(&buf.borrow()).unwrap());
}

#[inline(never)]
pub fn send_datagram2<RT: RuntimeTrait>(src: &Runtime, dst: &Peer<RT>) {
    must_let!(let Some(event) = src.pop_event());
    must_let!(let Event::Transmit(ref buf) = &*event);
    dst.receive_datagram(ipv4::Datagram::attach(&buf.borrow()).unwrap());
}

#[inline(never)]
pub fn poll_alice<F: Future>(f: Pin<&mut F>, ctx: &mut Context) -> Poll<F::Output> {
    Future::poll(f, ctx)
}

#[inline(never)]
pub fn poll_bob<F: Future>(f: Pin<&mut F>, ctx: &mut Context) -> Poll<F::Output> {
    Future::poll(f, ctx)
}

#[inline(never)]
pub fn one_send_recv_round<RT: RuntimeTrait>(
    ctx: &mut Context,
    buf: Bytes,

    alice_rt: &Runtime,
    alice_peer: &mut Peer<RT>,
    alice_fd: u16,

    bob_rt: &Runtime,
    bob_peer: &mut Peer<RT>,
    bob_fd: u16,
)
{
    // Send data from Alice to Bob
    alice_peer.send(alice_fd, buf.clone()).unwrap();
    assert!(poll_alice(Pin::new(alice_peer), ctx).is_pending());
    send_datagram1(&alice_rt, &bob_peer);

    // Receive it on Bob's side.
    must_let!(let Ok(Some(buf)) = bob_peer.recv(bob_fd));
    // assert_eq!(buf.len(), size);

    // Send data from Bob to Alice
    bob_peer.send(bob_fd, buf.clone()).unwrap();
    assert!(poll_bob(Pin::new(bob_peer), ctx).is_pending());
    send_datagram2(&bob_rt, &alice_peer);

    // Receive it on Alice's side.
    must_let!(let Ok(Some(_)) = alice_peer.recv(alice_fd));
    // assert_eq!(buf.len(), size);
}


pub fn main() {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let now = Instant::now();

    let alice_ipv4_addr = Ipv4Addr::new(192, 168, 1, 1);
    let alice_link_addr = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);
    let bob_link_addr = MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);
    let bob_ipv4_addr = Ipv4Addr::new(192, 168, 1, 2);

    let alice_options = Options {
        arp: arp::Options::default(),
        my_ipv4_addr: alice_ipv4_addr,
        my_link_addr: alice_link_addr,
        rng_seed: Seed::default(),
        tcp: tcp::Options::default(),
    };
    let alice_rt = Runtime::from_options(now, alice_options);
    let alice_arp = arp::Peer::new(now, alice_rt.clone()).unwrap();
    alice_arp.insert(bob_ipv4_addr, bob_link_addr);
    let mut alice_peer = Peer::new(alice_rt.clone(), alice_arp);

    let bob_options = Options {
        arp: arp::Options::default(),
        my_ipv4_addr: bob_ipv4_addr,
        my_link_addr: bob_link_addr,
        rng_seed: Seed::default(),
        tcp: tcp::Options::default(),
    };
    let bob_rt = Runtime::from_options(now, bob_options);
    let bob_arp = arp::Peer::new(now, bob_rt.clone()).unwrap();
    bob_arp.insert(alice_ipv4_addr, alice_link_addr);
    let mut bob_peer = Peer::new(bob_rt.clone(), bob_arp);

    // Establish the connection between the two peers.
    let listen_port = ip::Port::try_from(80).unwrap();
    let listen_addr = ipv4::Endpoint::new(bob_ipv4_addr, listen_port);
    let listen_fd = bob_peer.listen(listen_addr.clone(), 1).unwrap();

    let mut alice_connect_future = alice_peer.connect(listen_addr);

    // Send the SYN from Alice to Bob
    assert!(Future::poll(Pin::new(&mut alice_peer), &mut ctx).is_pending());
    send_datagram(&alice_rt, &bob_peer);

    // Send the SYN+ACK from Bob to Alice
    assert!(Future::poll(Pin::new(&mut bob_peer), &mut ctx).is_pending());
    send_datagram(&bob_rt, &alice_peer);

    // Send the ACK from Alice to Bob
    send_datagram(&alice_rt, &bob_peer);

    must_let!(let Ok(Some(bob_fd)) = bob_peer.accept(listen_fd));
    must_let!(let Poll::Ready(Ok(alice_fd)) = Future::poll(Pin::new(&mut alice_connect_future), &mut ctx));

    let size = 32;
    let buf = BytesMut::from(&vec![0u8; size][..]).freeze();

    // Send data from Alice to Bob
    alice_peer.send(alice_fd, buf.clone()).unwrap();
    assert!(Future::poll(Pin::new(&mut alice_peer), &mut ctx).is_pending());
    send_datagram(&alice_rt, &bob_peer);

    // Receive it on Bob's side.
    must_let!(let Ok(Some(buf)) = bob_peer.recv(bob_fd));
    assert_eq!(buf.len(), size);

    for _ in 0..250_000 {
        one_send_recv_round(
            &mut ctx,
            buf.clone(),
            &alice_rt,
            &mut alice_peer,
            alice_fd,
            &bob_rt,
            &mut bob_peer,
            bob_fd,
        );
    }
    println!("done");

}
