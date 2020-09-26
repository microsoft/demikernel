use catnip::protocols::tcp2::runtime::Runtime;
use catnip::protocols::ethernet2::MacAddress;
use catnip::protocols::{arp, tcp};
use catnip::runtime::Timer;
use std::cell::RefCell;
use std::rc::Rc;
use std::net::Ipv4Addr;
use std::time::Duration;
use std::time::Instant;
use catnip::runtime::TimerPtr;

#[derive(Clone)]
pub struct TimerRc(Rc<Timer<TimerRc>>);

impl TimerPtr for TimerRc {
    fn timer(&self) -> &Timer<Self> {
        &*self.0
    }
}

#[derive(Clone)]
pub struct LibOSRuntime {
    inner: Rc<RefCell<Inner>>,
}

impl LibOSRuntime {
    pub fn new(link_addr: MacAddress, ipv4_addr: Ipv4Addr) -> Self {
        let now = Instant::now();
        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            link_addr,
            ipv4_addr,
            rng: 1,
            arp_options: arp::Options::default(),
            tcp_options: tcp::Options::default(),
        };
        Self {
            inner: Rc::new(RefCell::new(inner))
        }
    }
}

struct Inner {
    timer: TimerRc,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    rng: u32,
    arp_options: arp::Options,
    tcp_options: tcp::Options,
}

impl Runtime for LibOSRuntime {
    fn transmit(&self, buf: Rc<RefCell<Vec<u8>>>) {
        todo!();
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

    type WaitFuture = catnip::runtime::WaitFuture<TimerRc>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let self_ = self.inner.borrow_mut();
        let now = self_.timer.0.now();
        self_.timer.0.wait_until(self_.timer.clone(), now + duration)
    }
    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        let self_ = self.inner.borrow_mut();
        self_.timer.0.wait_until(self_.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.inner.borrow().timer.0.now()
    }

    fn rng_gen_u32(&self) -> u32 {
        let mut self_ = self.inner.borrow_mut();
        let r = self_.rng;
        self_.rng += 1;
        r
    }
}
