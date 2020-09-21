use criterion::{black_box, criterion_group, criterion_main, Criterion};
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
use futures::Future;
use std::pin::Pin;
use std::convert::TryFrom;
use catnip::protocols::tcp2::peer::Peer;

pub fn criterion_benchmark(c: &mut Criterion) {
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
    let mut alice_rt = Runtime::from_options(now, alice_options);
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
    let mut bob_rt = Runtime::from_options(now, bob_options);
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
    must_let!(let Some(event) = alice_rt.pop_event());
    must_let!(let Event::Transmit(ref buf) = &*event);
    bob_peer.receive_datagram(ipv4::Datagram::attach(&buf.borrow()).unwrap());

    // Send the SYN+ACK from Bob to Alice
    assert!(Future::poll(Pin::new(&mut bob_peer), &mut ctx).is_pending());
    must_let!(let Some(event) = bob_rt.pop_event());
    must_let!(let Event::Transmit(ref buf) = &*event);
    alice_peer.receive_datagram(ipv4::Datagram::attach(&buf.borrow()).unwrap());

    // Send the ACK from Alice to Bob
    must_let!(let Some(event) = alice_rt.pop_event());
    must_let!(let Event::Transmit(ref buf) = &*event);
    bob_peer.receive_datagram(ipv4::Datagram::attach(&buf.borrow()).unwrap());

    must_let!(let Ok(Some(bob_fd)) = bob_peer.accept(listen_fd));
    must_let!(let Poll::Ready(Ok(alice_fd)) = Future::poll(Pin::new(&mut alice_connect_future), &mut ctx));



    // let size = 1024;
    // let buf = vec![0u8; size];


    // c.bench_function("fib 20", |b| b.iter(|| fibonacci(black_box(20))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
