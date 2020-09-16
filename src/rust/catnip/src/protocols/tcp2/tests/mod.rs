use crate::protocols::{arp, tcp, ip, ipv4};

use must_let::must_let;
use std::pin::Pin;
use std::task::{Poll, Context};
use std::future::Future;
use futures::task::noop_waker_ref;
use crate::protocols::tcp2::peer::{Peer, Runtime};
use std::time::Instant;
use crate::runtime::Timer;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder};
use std::net::Ipv4Addr;
use crate::protocols::ethernet2::MacAddress;
use std::time::Duration;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::convert::TryFrom;

struct TestRuntime {
    name: &'static str,
    timer: Timer,
    rng: u32,
    outgoing: VecDeque<Vec<u8>>,

    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    tcp_options: tcp::Options,
}

impl TestRuntime {
    fn new(name: &'static str, now: Instant, link_addr: MacAddress, ipv4_addr: Ipv4Addr) -> Rc<RefCell<Self>> {
        let self_ = Self {
            name,
            timer: Timer::new(now),
            rng: 1,
            outgoing: VecDeque::new(),
            link_addr,
            ipv4_addr,
            tcp_options: tcp::Options::default(),
        };
        Rc::new(RefCell::new(self_))
    }
}

impl Runtime for Rc<RefCell<TestRuntime>> {
    fn transmit(&self, buf: &[u8]) {
        let datagram = ipv4::Datagram::attach(buf).unwrap();
        let decoder = TcpSegmentDecoder::try_from(datagram).unwrap();
        let segment = TcpSegment::try_from(decoder).unwrap();
        self.borrow_mut().outgoing.push_back(buf.to_owned());
    }

    fn local_link_addr(&self) -> MacAddress {
        self.borrow().link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.borrow().ipv4_addr.clone()
    }

    fn tcp_options(&self) -> tcp::Options {
        self.borrow().tcp_options.clone()
    }

    type WaitFuture = crate::runtime::WaitFuture;
    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let mut self_ = self.borrow_mut();
        let now = self_.timer.now();
        self_.timer.wait_until(now + duration)
    }
    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        self.borrow_mut().timer.wait_until(when)
    }

    fn now(&self) -> Instant {
        self.borrow().timer.now()
    }

    fn rng_gen_u32(&self) -> u32 {
        let mut self_ = self.borrow_mut();
        let r = self_.rng;
        self_.rng += 1;
        r
    }
}

struct TestParticipant {
    rt: Rc<RefCell<TestRuntime>>,
    peer: Peer<Rc<RefCell<TestRuntime>>>,
    addr: Ipv4Addr,
}

impl TestParticipant {
    fn poll(&mut self) {
        let mut ctx = Context::from_waker(noop_waker_ref());
        assert!(Future::poll(Pin::new(&mut self.peer), &mut ctx).is_pending());
    }

    fn pop(&self) -> Vec<u8> {
        self.rt.borrow_mut().outgoing.pop_front().unwrap()
    }

    fn push(&self, buf: Vec<u8>) {
        let datagram = ipv4::Datagram::attach(&buf[..]).unwrap();
        self.peer.receive_datagram(datagram);
    }
}

struct Test {
    arp: arp::Peer,

    alice: TestParticipant,
    bob: TestParticipant,
}

impl Test {
    fn new() -> Self {
        // TODO: Determinize this better.
        let now = Instant::now();

        let alice_mac = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);
        let alice_ipv4 = Ipv4Addr::new(192, 168, 1, 1);
        let alice_rt = TestRuntime::new("alice", now, alice_mac, alice_ipv4);

        let bob_mac = MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);
        let bob_ipv4 = Ipv4Addr::new(192, 168, 1, 2);
        let bob_rt = TestRuntime::new("bob", now, bob_mac, bob_ipv4);

        // TODO: Remove this dependency on Runtime
        let rt0 = crate::runtime::Runtime::from_options(now, crate::options::Options::default());
        let arp = arp::Peer::new(now, rt0).unwrap();
        arp.insert(alice_ipv4, alice_mac);
        arp.insert(bob_ipv4, bob_mac);

        Self {
            arp: arp.clone(),
            alice: TestParticipant {
                addr: alice_ipv4,
                rt: alice_rt.clone(),
                peer: Peer::new(alice_rt, arp.clone()),
            },
            bob: TestParticipant {
                addr: bob_ipv4,
                rt: bob_rt.clone(),
                peer: Peer::new(bob_rt, arp.clone()),
            },
        }
    }
}

#[test]
fn test_connect() {
    let mut test = Test::new();

    let listen_port = ip::Port::try_from(80).unwrap();
    let listen_addr = ipv4::Endpoint::new(test.alice.addr, listen_port);
    let listen_fd = test.alice.peer.listen(listen_addr.clone(), 1).unwrap();

    let bob_fd = test.bob.peer.connect(listen_addr).unwrap();
    // Sending the SYN is background work.
    test.bob.poll();
    test.alice.push(test.bob.pop());

    // As is replying with the SYN+ACK
    test.alice.poll();
    test.bob.push(test.alice.pop());

    // But sending the final ACK happens immediately
    test.alice.push(test.bob.pop());

    must_let!(let Ok(Some(alice_fd)) = test.alice.peer.accept(listen_fd));
    must_let!(let Ok(true) = test.bob.peer.connect_finished(bob_fd));

    test.bob.peer.send(bob_fd, vec![1, 2, 3, 4]).unwrap();
    test.bob.poll();
    test.alice.push(test.bob.pop());

    must_let!(let Ok(Some(buf)) = test.alice.peer.recv(alice_fd));
    assert_eq!(buf, vec![1, 2, 3, 4]);

    test.alice.peer.send(alice_fd, vec![5]).unwrap();
    test.alice.poll();

    test.bob.push(test.alice.pop());
    must_let!(let Ok(Some(buf)) = test.bob.peer.recv(bob_fd));
    assert_eq!(buf, vec![5]);
}
