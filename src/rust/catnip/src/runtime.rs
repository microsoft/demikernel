// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use crate::protocols::{arp, tcp};
use std::net::Ipv4Addr;
use crate::protocols::ethernet::MacAddress;
use std::rc::Rc;
use std::cell::RefCell;
use std::future::Future;
use std::time::{Duration, Instant};
use rand::distributions::{Standard, Distribution};

pub trait Runtime: Clone + Unpin + 'static {
    fn advance_clock(&self, now: Instant);
    fn transmit(&self, buf: Rc<RefCell<Vec<u8>>>);

    fn local_link_addr(&self) -> MacAddress;
    fn local_ipv4_addr(&self) -> Ipv4Addr;
    fn arp_options(&self) -> arp::Options;
    fn tcp_options(&self) -> tcp::Options;

    type WaitFuture: Future<Output = ()>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture;
    fn wait_until(&self, when: Instant) -> Self::WaitFuture;
    fn now(&self) -> Instant;

    fn rng_gen<T>(&self) -> T where Standard: Distribution<T>;
}
