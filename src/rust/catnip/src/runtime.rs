// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use crate::{
    protocols::{
        arp,
        ethernet2::MacAddress,
        tcp,
    },
    scheduler::{
        Operation,
        Scheduler,
        SchedulerHandle,
    },
    sync::Bytes,
};
use rand::distributions::{
    Distribution,
    Standard,
};
use std::{
    future::Future,
    net::Ipv4Addr,
    time::{
        Duration,
        Instant,
    },
};

pub trait PacketBuf {
    fn compute_size(&self) -> usize;
    fn serialize(&self, buf: &mut [u8]);
}

pub trait Runtime: Clone + Unpin + 'static {
    fn advance_clock(&self, now: Instant);
    fn transmit(&self, pkt: impl PacketBuf);
    fn receive(&self) -> Option<Bytes>;

    fn local_link_addr(&self) -> MacAddress;
    fn local_ipv4_addr(&self) -> Ipv4Addr;
    fn arp_options(&self) -> arp::Options;
    fn tcp_options(&self) -> tcp::Options;

    type WaitFuture: Future<Output = ()>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture;
    fn wait_until(&self, when: Instant) -> Self::WaitFuture;
    fn now(&self) -> Instant;

    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>;

    fn spawn<F: Future<Output = ()> + 'static>(&self, future: F) -> SchedulerHandle;
    fn scheduler(&self) -> &Scheduler<Operation<Self>>;
}
