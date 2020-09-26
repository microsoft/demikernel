use crate::protocols::{arp, tcp, ip, ipv4};

use bytes::{Bytes, BytesMut};
use must_let::must_let;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::future::Future;
use futures::task::noop_waker_ref;
use crate::protocols::tcp2::peer::Peer;
use crate::protocols::tcp2::runtime::Runtime;
use std::time::Instant;
use crate::runtime::Timer;
use std::net::Ipv4Addr;
use crate::protocols::ethernet2::MacAddress;
use std::time::Duration;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::convert::TryFrom;
use crate::runtime::TimerPtr;

#[derive(Clone)]
struct TimerRc(Rc<Timer<TimerRc>>);

impl TimerPtr for TimerRc {
    fn timer(&self) -> &Timer<Self> {
        &*self.0
    }
}

struct TestRuntime {
    #[allow(unused)]
    name: &'static str,
    timer: TimerRc,
    rng: u32,
    outgoing: VecDeque<Vec<u8>>,

    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    tcp_options: tcp::Options,
    arp_options: arp::Options,
}

impl TestRuntime {
    fn new(name: &'static str, now: Instant, link_addr: MacAddress, ipv4_addr: Ipv4Addr) -> Rc<RefCell<Self>> {
        let self_ = Self {
            name,
            timer: TimerRc(Rc::new(Timer::new(now))),
            rng: 1,
            outgoing: VecDeque::new(),
            link_addr,
            ipv4_addr,
            tcp_options: tcp::Options::default(),
            arp_options: arp::Options::default(),
        };
        Rc::new(RefCell::new(self_))
    }
}

impl Runtime for Rc<RefCell<TestRuntime>> {
    fn transmit(&self, buf: Rc<RefCell<Vec<u8>>>) {
        self.borrow_mut().outgoing.push_back(buf.borrow_mut().clone());
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

    fn arp_options(&self) -> arp::Options {
        self.borrow().arp_options.clone()
    }

    fn advance_clock(&self, now: Instant) {
        self.borrow_mut().timer.0.advance_clock(now);
    }

    type WaitFuture = crate::runtime::WaitFuture<TimerRc>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let self_ = self.borrow_mut();
        let now = self_.timer.0.now();
        self_.timer.0.wait_until(self_.timer.clone(), now + duration)
    }
    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        let self_ = self.borrow_mut();
        self_.timer.0.wait_until(self_.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.borrow().timer.0.now()
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
    fn advance(&mut self, duration: Duration) {
        let rt = self.rt.borrow_mut();
        let now = rt.timer.0.now();
        rt.timer.0.advance_clock(now + duration);
    }

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
    #[allow(unused)]
    arp: arp::Peer<Rc<RefCell<TestRuntime>>>,

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

        let arp = arp::Peer::new(now, alice_rt.clone()).unwrap();
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

fn bytes(s: &[u8]) -> Bytes {
    BytesMut::from(s).freeze()
}

#[test]
fn test_connect() {
    let mut test = Test::new();
    let mut ctx = Context::from_waker(noop_waker_ref());

    let listen_port = ip::Port::try_from(80).unwrap();
    let listen_addr = ipv4::Endpoint::new(test.alice.addr, listen_port);
    let listen_fd = test.alice.peer.listen(listen_addr.clone(), 1).unwrap();

    let mut bob_connect_future = test.bob.peer.connect(listen_addr);
    assert!(Future::poll(Pin::new(&mut bob_connect_future), &mut ctx).is_pending());

    // Sending the SYN is background work.
    test.bob.poll();
    test.alice.push(test.bob.pop());

    // As is replying with the SYN+ACK
    test.alice.poll();
    test.bob.push(test.alice.pop());

    // But sending the final ACK happens immediately
    test.alice.push(test.bob.pop());

    must_let!(let Ok(Some(alice_fd)) = test.alice.peer.accept(listen_fd));
    must_let!(let Poll::Ready(Ok(bob_fd)) = Future::poll(Pin::new(&mut bob_connect_future), &mut ctx));

    test.bob.peer.send(bob_fd, bytes(&[1u8, 2, 3, 4])).unwrap();
    test.bob.poll();
    test.alice.push(test.bob.pop());

    must_let!(let Ok(Some(buf)) = test.alice.peer.recv(alice_fd));
    assert_eq!(buf, vec![1, 2, 3, 4]);

    test.alice.peer.send(alice_fd, bytes(&[5])).unwrap();
    test.alice.poll();

    test.bob.push(test.alice.pop());
    must_let!(let Ok(Some(buf)) = test.bob.peer.recv(bob_fd));
    assert_eq!(buf, vec![5]);

    // Send a segment from Bob to Alice but drop it, checking to see that it gets retransmitted.
    test.bob.peer.send(bob_fd, bytes(&[5, 6, 7, 8])).unwrap();
    test.bob.poll();
    let _ = test.bob.pop();

    must_let!(let Ok(r) = test.alice.peer.recv(alice_fd));
    assert!(r.is_none());

    // Advance Bob's timer past the retransmit deadline
    let rto = test.bob.peer.current_rto(bob_fd).unwrap();
    test.bob.advance(rto);
    test.bob.poll();

    // Deliver the retransmitted segment to Alice.
    test.alice.push(test.bob.pop());

    must_let!(let Ok(Some(buf)) = test.alice.peer.recv(alice_fd));
    assert_eq!(buf, vec![5, 6, 7, 8]);
}
