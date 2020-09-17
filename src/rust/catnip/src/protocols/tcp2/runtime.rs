use crate::protocols::{arp, ip, ipv4, tcp};
use std::net::Ipv4Addr;
use std::task::{Poll, Context};
use std::collections::hash_map::Entry;
use crate::protocols::ethernet2::MacAddress;
use crate::protocols::tcp::peer::isn_generator::IsnGenerator;
use crate::protocols::tcp::segment::{TcpSegment, TcpSegmentDecoder, TcpSegmentEncoder};
use crate::fail::Fail;
use crate::event::Event;
use std::convert::TryFrom;
use std::collections::{VecDeque, HashMap};
use std::num::Wrapping;
use futures_intrusive::channel::LocalChannel;
use std::rc::Rc;
use std::cell::RefCell;
use futures::channel::oneshot;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use futures::stream::FuturesUnordered;
use futures::FutureExt;
use rand::Rng;

pub trait Runtime: Clone {
    fn transmit(&self, buf: &[u8]);

    fn local_link_addr(&self) -> MacAddress;
    fn local_ipv4_addr(&self) -> Ipv4Addr;
    fn tcp_options(&self) -> tcp::Options;

    type WaitFuture: Future<Output = ()>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture;
    fn wait_until(&self, when: Instant) -> Self::WaitFuture;
    fn now(&self) -> Instant;

    fn rng_gen_u32(&self) -> u32;
}

impl Runtime for crate::runtime::Runtime {
    fn transmit(&self, buf: &[u8]) {
        let event = crate::event::Event::Transmit(Rc::new(RefCell::new(buf.to_vec())));
        self.emit_event(event);
    }

    fn local_link_addr(&self) -> MacAddress {
        self.options().my_link_addr
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.options().my_ipv4_addr
    }

    fn tcp_options(&self) -> tcp::Options {
        self.options().tcp
    }

    type WaitFuture = crate::runtime::WaitFuture;
    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        crate::runtime::Runtime::wait(self, duration)
    }

    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        crate::runtime::Runtime::wait_until(self, when)
    }

    fn now(&self) -> Instant {
        crate::runtime::Runtime::now(self)
    }

    fn rng_gen_u32(&self) -> u32 {
        self.with_rng(|r| r.gen())
    }
}
