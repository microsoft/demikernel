// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::protocols::{arp, tcp};
use crate::engine::Engine;
use crate::runtime::Runtime;
use std::time::Instant;
use crate::timer::Timer;
use std::net::Ipv4Addr;
use crate::protocols::ethernet::MacAddress;
use std::time::Duration;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;
use crate::timer::TimerRc;
use rand_chacha::ChaChaRng;
use rand::{Rng, SeedableRng};
use rand::distributions::{Standard, Distribution};

pub const RECEIVE_WINDOW_SIZE: usize = 1024;
pub const ALICE_MAC: MacAddress = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xab]);
pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
pub const BOB_MAC: MacAddress = MacAddress::new([0xab, 0x89, 0x67, 0x45, 0x23, 0x12]);
pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);
pub const CARRIE_MAC: MacAddress = MacAddress::new([0xef, 0xcd, 0xab, 0x89, 0x67, 0x45]);
pub const CARRIE_IPV4: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 3);

#[derive(Clone)]
pub struct TestRuntime {
    inner: Rc<RefCell<Inner>>,
}

impl TestRuntime {
    pub fn new(name: &'static str, now: Instant, link_addr: MacAddress, ipv4_addr: Ipv4Addr) -> Self {
        let inner = Inner {
            name,
            timer: TimerRc(Rc::new(Timer::new(now))),
            rng: ChaChaRng::from_seed([0; 32]),
            outgoing: VecDeque::new(),
            link_addr,
            ipv4_addr,
            tcp_options: tcp::Options::default(),
            arp_options: arp::Options::default(),
        };
        Self { inner: Rc::new(RefCell::new(inner)) }
    }

    pub fn pop_frame(&self) -> Vec<u8> {
        self.inner.borrow_mut().outgoing.pop_front().unwrap()
    }
}

struct Inner {
    #[allow(unused)]
    name: &'static str,
    timer: TimerRc,
    rng: ChaChaRng,
    outgoing: VecDeque<Vec<u8>>,

    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    tcp_options: tcp::Options,
    arp_options: arp::Options,
}


impl Runtime for TestRuntime {
    fn transmit(&self, buf: Rc<RefCell<Vec<u8>>>) {
        self.inner.borrow_mut().outgoing.push_back(buf.borrow_mut().clone());
    }

    fn local_link_addr(&self) -> MacAddress {
        self.inner.borrow().link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.inner.borrow().ipv4_addr.clone()
    }

    fn tcp_options(&self) -> tcp::Options {
        self.inner.borrow().tcp_options.clone()
    }

    fn arp_options(&self) -> arp::Options {
        self.inner.borrow().arp_options.clone()
    }

    fn advance_clock(&self, now: Instant) {
        self.inner.borrow_mut().timer.0.advance_clock(now);
    }

    type WaitFuture = crate::timer::WaitFuture<TimerRc>;

    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let inner = self.inner.borrow_mut();
        let now = inner.timer.0.now();
        inner.timer.0.wait_until(inner.timer.clone(), now + duration)
    }
    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        let inner = self.inner.borrow_mut();
        inner.timer.0.wait_until(inner.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.inner.borrow().timer.0.now()
    }

    fn rng_gen<T>(&self) -> T where Standard: Distribution<T> {
        let mut inner = self.inner.borrow_mut();
        inner.rng.gen()
    }
}

pub fn new_alice(now: Instant) -> Engine<TestRuntime> {
    let rt = TestRuntime::new("alice", now, ALICE_MAC, ALICE_IPV4);
    Engine::new(rt).unwrap()
}

pub fn new_bob(now: Instant) -> Engine<TestRuntime> {
    let rt = TestRuntime::new("bob", now, BOB_MAC, BOB_IPV4);
    Engine::new(rt).unwrap()
}

pub fn new_carrie(now: Instant) -> Engine<TestRuntime> {
    let rt = TestRuntime::new("carrie", now, CARRIE_MAC, CARRIE_IPV4);
    Engine::new(rt).unwrap()
}
