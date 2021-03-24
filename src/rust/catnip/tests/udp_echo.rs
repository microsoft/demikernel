#![feature(const_fn, const_panic, const_alloc_layout)]
#![feature(const_mut_refs, const_type_name)]
#![feature(new_uninit)]

use arrayvec::ArrayVec;
use std::ptr;
use std::mem;
use std::slice;
use catnip::{
    interop::{
        dmtr_opcode_t,
        dmtr_sgarray_t,
        dmtr_sgaseg_t,
    },
    libos::LibOS,
    protocols::{
        arp,
        ethernet2::MacAddress,
        ip,
        ipv4,
        udp,
        tcp,
    },
    runtime::{
        RECEIVE_BATCH_SIZE,
        PacketBuf,
        Runtime,
    },
    scheduler::{
        Operation,
        Scheduler,
        SchedulerHandle,
    },
    sync::{
        Bytes,
        BytesMut,
    },
    test_helpers::{
        ALICE_IPV4,
        ALICE_MAC,
        BOB_IPV4,
        BOB_MAC,
    },
    timer::{
        Timer,
        TimerRc,
    },
};
use crossbeam_channel;
use futures::FutureExt;
use libc;
use rand::{
    distributions::{
        Distribution,
        Standard,
    },
    rngs::SmallRng,
    seq::SliceRandom,
    Rng,
    SeedableRng,
};
use std::{
    cell::RefCell,
    convert::TryFrom,
    env,
    future::Future,
    net::Ipv4Addr,
    rc::Rc,
    thread,
    time::{
        Duration,
        Instant,
    },
};
use tracy_client::static_span;

#[derive(Clone)]
pub struct TestRuntime {
    inner: Rc<RefCell<Inner>>,
    scheduler: Scheduler<Operation<TestRuntime>>,
}

impl TestRuntime {
    pub fn new(
        now: Instant,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        incoming: crossbeam_channel::Receiver<Bytes>,
        outgoing: crossbeam_channel::Sender<Bytes>,
    ) -> Self {
        let mut arp_options = arp::Options::default();
        arp_options.retry_count = 2;
        arp_options.cache_ttl = Duration::from_secs(600);
        arp_options.request_timeout = Duration::from_secs(1);
        arp_options.initial_values.insert(ALICE_MAC, ALICE_IPV4);
        arp_options.initial_values.insert(BOB_MAC, BOB_IPV4);

        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            rng: SmallRng::from_seed([0; 16]),
            incoming,
            outgoing,
            link_addr,
            ipv4_addr,
            tcp_options: tcp::Options::default(),
            arp_options,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::new(),
        }
    }
}

struct Inner {
    timer: TimerRc,
    rng: SmallRng,
    incoming: crossbeam_channel::Receiver<Bytes>,
    outgoing: crossbeam_channel::Sender<Bytes>,

    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    tcp_options: tcp::Options,
    arp_options: arp::Options,
}

impl Runtime for TestRuntime {
    type WaitFuture = catnip::timer::WaitFuture<TimerRc>;
    type Buf = Bytes;

    fn into_sgarray(&self, buf: Bytes) -> dmtr_sgarray_t {
        let buf_copy: Box<[u8]> = (&buf[..]).into();
        let ptr = Box::into_raw(buf_copy);
        let sgaseg = dmtr_sgaseg_t {
            sgaseg_buf: ptr as *mut _,
            sgaseg_len: buf.len() as u32,
        };
        dmtr_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        }
    }

    fn alloc_sgarray(&self, size: usize) -> dmtr_sgarray_t {
        let allocation: Box<[u8]> = unsafe { Box::new_uninit_slice(size).assume_init() };
        let ptr = Box::into_raw(allocation);
        let sgaseg = dmtr_sgaseg_t {
            sgaseg_buf: ptr as *mut _,
            sgaseg_len: size as u32,
        };
        dmtr_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        }
    }

    fn free_sgarray(&self, sga: dmtr_sgarray_t) {
        assert_eq!(sga.sga_numsegs, 1);
        for i in 0..sga.sga_numsegs as usize {
            let seg = &sga.sga_segs[i];
            let allocation: Box<[u8]> = unsafe {
                Box::from_raw(slice::from_raw_parts_mut(
                    seg.sgaseg_buf as *mut _,
                    seg.sgaseg_len as usize,
                ))
            };
            drop(allocation);
        }
    }

    fn clone_sgarray(&self, sga: &dmtr_sgarray_t) -> Bytes {
        let mut len = 0;
        for i in 0..sga.sga_numsegs as usize {
            len += sga.sga_segs[i].sgaseg_len;
        }
        let mut buf = BytesMut::zeroed(len as usize);
        let mut pos = 0;
        for i in 0..sga.sga_numsegs as usize {
            let seg = &sga.sga_segs[i];
            let seg_slice = unsafe {
                slice::from_raw_parts(seg.sgaseg_buf as *mut u8, seg.sgaseg_len as usize)
            };
            buf[pos..(pos + seg_slice.len())].copy_from_slice(seg_slice);
            pos += seg_slice.len();
        }
        buf.freeze()
    }

    fn transmit(&self, pkt: impl PacketBuf<Bytes>) {
        let _s = static_span!();
        let header_size = pkt.header_size();
        let body_size = pkt.body_size();

        let mut buf = BytesMut::zeroed(header_size + body_size);
        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }
        self.inner
            .borrow_mut()
            .outgoing
            .try_send(buf.freeze())
            .unwrap();
    }

    fn receive(&self) -> ArrayVec<[Bytes; RECEIVE_BATCH_SIZE]> {
        let _s = static_span!();
        let mut out = ArrayVec::new();
        if let Some(buf) = self.inner.borrow_mut().incoming.try_recv().ok() {
            out.push(buf);
        }
        out
    }

    fn scheduler(&self) -> &Scheduler<Operation<Self>> {
        &self.scheduler
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

    fn udp_options(&self) -> udp::Options {
        udp::Options::default()
    }

    fn arp_options(&self) -> arp::Options {
        self.inner.borrow().arp_options.clone()
    }

    fn advance_clock(&self, now: Instant) {
        self.inner.borrow_mut().timer.0.advance_clock(now);
    }

    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let inner = self.inner.borrow_mut();
        let now = inner.timer.0.now();
        inner
            .timer
            .0
            .wait_until(inner.timer.clone(), now + duration)
    }

    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        let inner = self.inner.borrow_mut();
        inner.timer.0.wait_until(inner.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.inner.borrow().timer.0.now()
    }

    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        let mut inner = self.inner.borrow_mut();
        inner.rng.gen()
    }

    fn rng_shuffle<T>(&self, slice: &mut [T]) {
        let mut inner = self.inner.borrow_mut();
        slice.shuffle(&mut inner.rng);
    }

    fn spawn<F: Future<Output = ()> + 'static>(&self, future: F) -> SchedulerHandle {
        self.scheduler
            .insert(Operation::Background(future.boxed_local()))
    }
}

#[test]
// #[cfg(not(feature = "threadunsafe"))]
fn udp_echo() {
    let (forward_tx, forward_rx) = crossbeam_channel::unbounded();
    let (backward_tx, backward_rx) = crossbeam_channel::unbounded();

    let now = Instant::now();
    let port = ip::Port::try_from(80).unwrap();
    let alice_addr = ipv4::Endpoint::new(ALICE_IPV4, port);
    let bob_addr = ipv4::Endpoint::new(BOB_IPV4, port);

    let num_iters: usize = env::var("NUM_ITERS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1);
    let size = 32;
    let fill_char = 'a' as u8;
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);

    let client = thread::spawn(move || {
        let alice_rt = TestRuntime::new(now, ALICE_MAC, ALICE_IPV4, backward_rx, forward_tx);
        let mut alice = LibOS::new(alice_rt).unwrap();

        let alice_fd = alice.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
        alice.bind(alice_fd, alice_addr).unwrap();
        let qt = alice.connect(alice_fd, bob_addr);
        assert_eq!(alice.wait(qt).qr_opcode, dmtr_opcode_t::DMTR_OPC_CONNECT);

        let mut buf = BytesMut::zeroed(size);
        for a in &mut buf[..] {
            *a = fill_char;
        }
        let body_sga = alice.rt().into_sgarray(buf.freeze());

        let mut samples = Vec::with_capacity(num_iters);

        for _ in 0..num_iters {
            let start = Instant::now();

            let qt = alice.push(alice_fd, &body_sga);
            assert_eq!(alice.wait(qt).qr_opcode, dmtr_opcode_t::DMTR_OPC_PUSH);

            let qt = alice.pop(alice_fd);
            let qr = alice.wait(qt);
            assert_eq!(qr.qr_opcode, dmtr_opcode_t::DMTR_OPC_POP);

            let sga = unsafe { qr.qr_value.sga };
            assert_eq!(sga.sga_numsegs, 1);
            assert_eq!(sga.sga_segs[0].sgaseg_len, size as u32);
            alice.rt().free_sgarray(sga);

            samples.push(start.elapsed());
        }

        alice.rt().free_sgarray(body_sga);
        done_tx.send(samples).unwrap();
    });

    let server = thread::spawn(move || {
        let bob_rt = TestRuntime::new(now, BOB_MAC, BOB_IPV4, forward_rx, backward_tx);
        let mut bob = LibOS::new(bob_rt).unwrap();

        let bob_fd = bob.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).unwrap();
        bob.bind(bob_fd, bob_addr).unwrap();
        let qt = bob.connect(bob_fd, alice_addr);
        assert_eq!(bob.wait(qt).qr_opcode, dmtr_opcode_t::DMTR_OPC_CONNECT);

        for _ in 0..num_iters {
            let qt = bob.pop(bob_fd);
            let qr = bob.wait(qt);
            assert_eq!(qr.qr_opcode, dmtr_opcode_t::DMTR_OPC_POP);

            let sga = unsafe { qr.qr_value.sga };
            assert_eq!(sga.sga_numsegs, 1);
            assert_eq!(sga.sga_segs[0].sgaseg_len, size as u32);

            let qt = bob.push(bob_fd, &sga);
            assert_eq!(bob.wait(qt).qr_opcode, dmtr_opcode_t::DMTR_OPC_PUSH);
            bob.rt().free_sgarray(sga);
        }
    });

    client.join().unwrap();
    server.join().unwrap();

    let samples = done_rx.recv().unwrap();
    let mut h = histogram::Histogram::new();
    for s in samples {
        h.increment(s.as_nanos() as u64).unwrap();
    }
    println!("Min:   {:?}", Duration::from_nanos(h.minimum().unwrap()));
    println!(
        "p25:   {:?}",
        Duration::from_nanos(h.percentile(0.25).unwrap())
    );
    println!(
        "p50:   {:?}",
        Duration::from_nanos(h.percentile(0.50).unwrap())
    );
    println!(
        "p75:   {:?}",
        Duration::from_nanos(h.percentile(0.75).unwrap())
    );
    println!(
        "p90:   {:?}",
        Duration::from_nanos(h.percentile(0.90).unwrap())
    );
    println!(
        "p95:   {:?}",
        Duration::from_nanos(h.percentile(0.95).unwrap())
    );
    println!(
        "p99:   {:?}",
        Duration::from_nanos(h.percentile(0.99).unwrap())
    );
    println!(
        "p99.9: {:?}",
        Duration::from_nanos(h.percentile(0.999).unwrap())
    );
    println!("Max:   {:?}", Duration::from_nanos(h.maximum().unwrap()));
}
