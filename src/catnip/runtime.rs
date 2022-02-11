// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::catnip::memory::{
    DPDKBuf,
    Mbuf,
    MemoryManager,
};
use ::arrayvec::ArrayVec;
use ::catnip::{
    protocols::ethernet2::MIN_PAYLOAD_SIZE,
    timer::{
        Timer,
        TimerPtr,
        WaitFuture,
    },
};
use ::catwalk::{
    Scheduler,
    SchedulerFuture,
    SchedulerHandle,
};
use ::dpdk_rs::{
    rte_eth_rx_burst,
    rte_eth_tx_burst,
    rte_mbuf,
    rte_pktmbuf_chain,
};
use ::rand::{
    distributions::{
        Distribution,
        Standard,
    },
    rngs::SmallRng,
    seq::SliceRandom,
    Rng,
    SeedableRng,
};
use ::runtime::{
    memory::MemoryRuntime,
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        consts::RECEIVE_BATCH_SIZE,
        types::MacAddress,
        NetworkRuntime,
        PacketBuf,
    },
    task::SchedulerRuntime,
    types::dmtr_sgarray_t,
    utils::UtilsRuntime,
    Runtime,
};
use ::std::{
    cell::RefCell,
    collections::HashMap,
    mem,
    net::Ipv4Addr,
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

#[cfg(feature = "profiler")]
use perftools::timer;

#[derive(Clone)]
pub struct TimerRc(Rc<Timer<TimerRc>>);

impl TimerPtr for TimerRc {
    fn timer(&self) -> &Timer<Self> {
        &*self.0
    }
}

#[derive(Clone)]
pub struct DPDKRuntime {
    inner: Rc<RefCell<Inner>>,
    scheduler: Scheduler,
}

impl DPDKRuntime {
    pub fn new(
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        dpdk_port_id: u16,
        memory_manager: MemoryManager,
        arp_table: HashMap<Ipv4Addr, MacAddress>,
        disable_arp: bool,
        mss: usize,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let rng = SmallRng::from_rng(&mut rng).expect("Failed to initialize RNG");
        let now = Instant::now();

        let arp_options = ArpConfig::new(
            Some(Duration::from_secs(15)),
            Some(Duration::from_secs(20)),
            Some(5),
            Some(arp_table),
            Some(disable_arp),
        );

        let tcp_options = TcpConfig::new(
            Some(mss),
            None,
            None,
            Some(0xffff),
            Some(0),
            None,
            Some(tcp_checksum_offload),
            Some(tcp_checksum_offload),
        );

        let udp_options = UdpConfig::new(Some(udp_checksum_offload), Some(udp_checksum_offload));

        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            link_addr,
            ipv4_addr,
            rng,
            arp_options,
            tcp_options,
            udp_options,

            dpdk_port_id,
            memory_manager,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::default(),
        }
    }

    pub fn alloc_body_mbuf(&self) -> Mbuf {
        self.inner.borrow().memory_manager.alloc_body_mbuf()
    }

    pub fn port_id(&self) -> u16 {
        self.inner.borrow().dpdk_port_id
    }

    pub fn memory_manager(&self) -> MemoryManager {
        self.inner.borrow().memory_manager.clone()
    }
}

struct Inner {
    timer: TimerRc,
    memory_manager: MemoryManager,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    rng: SmallRng,
    arp_options: ArpConfig,
    tcp_options: TcpConfig,
    udp_options: UdpConfig,

    dpdk_port_id: u16,
}

impl MemoryRuntime for DPDKRuntime {
    type Buf = DPDKBuf;

    fn into_sgarray(&self, buf: Self::Buf) -> dmtr_sgarray_t {
        self.inner.borrow().memory_manager.into_sgarray(buf)
    }

    fn alloc_sgarray(&self, size: usize) -> dmtr_sgarray_t {
        self.inner.borrow().memory_manager.alloc_sgarray(size)
    }

    fn free_sgarray(&self, sga: dmtr_sgarray_t) {
        self.inner.borrow().memory_manager.free_sgarray(sga)
    }

    fn clone_sgarray(&self, sga: &dmtr_sgarray_t) -> Self::Buf {
        self.inner.borrow().memory_manager.clone_sgarray(sga)
    }
}

impl NetworkRuntime for DPDKRuntime {
    fn transmit(&self, buf: impl PacketBuf<DPDKBuf>) {
        // Alloc header mbuf, check header size.
        // Serialize header.
        // Decide if we can inline the data --
        //   1) How much space is left?
        //   2) Is the body small enough?
        // If we can inline, copy and return.
        // If we can't inline...
        //   1) See if the body is managed => take
        //   2) Not managed => alloc body
        // Chain body buffer.

        // First, allocate a header mbuf and write the header into it.
        let inner = self.inner.borrow_mut();
        let mut header_mbuf = inner.memory_manager.alloc_header_mbuf();
        let header_size = buf.header_size();
        assert!(header_size <= header_mbuf.len());
        buf.write_header(unsafe { &mut header_mbuf.slice_mut()[..header_size] });

        if let Some(body) = buf.take_body() {
            // Next, see how much space we have remaining and inline the body if we have room.
            let inline_space = header_mbuf.len() - header_size;

            // Chain a buffer
            if body.len() > inline_space {
                assert!(header_size + body.len() >= MIN_PAYLOAD_SIZE);

                // We're only using the header_mbuf for, well, the header.
                header_mbuf.trim(header_mbuf.len() - header_size);

                let body_mbuf = match body {
                    DPDKBuf::Managed(mbuf) => mbuf,
                    DPDKBuf::External(bytes) => {
                        let mut mbuf = inner.memory_manager.alloc_body_mbuf();
                        assert!(mbuf.len() >= bytes.len());
                        unsafe { mbuf.slice_mut()[..bytes.len()].copy_from_slice(&bytes[..]) };
                        mbuf.trim(mbuf.len() - bytes.len());
                        mbuf
                    },
                };
                unsafe {
                    assert_eq!(
                        rte_pktmbuf_chain(header_mbuf.ptr(), body_mbuf.into_raw()),
                        0
                    );
                }
                let mut header_mbuf_ptr = header_mbuf.into_raw();
                let num_sent =
                    unsafe { rte_eth_tx_burst(inner.dpdk_port_id, 0, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
            // Otherwise, write in the inline space.
            else {
                let body_buf = unsafe {
                    &mut header_mbuf.slice_mut()[header_size..(header_size + body.len())]
                };
                body_buf.copy_from_slice(&body[..]);

                if header_size + body.len() < MIN_PAYLOAD_SIZE {
                    let padding_bytes = MIN_PAYLOAD_SIZE - (header_size + body.len());
                    let padding_buf = unsafe {
                        &mut header_mbuf.slice_mut()[(header_size + body.len())..][..padding_bytes]
                    };
                    for byte in padding_buf {
                        *byte = 0;
                    }
                }

                let frame_size = std::cmp::max(header_size + body.len(), MIN_PAYLOAD_SIZE);
                header_mbuf.trim(header_mbuf.len() - frame_size);

                let mut header_mbuf_ptr = header_mbuf.into_raw();
                let num_sent =
                    unsafe { rte_eth_tx_burst(inner.dpdk_port_id, 0, &mut header_mbuf_ptr, 1) };
                assert_eq!(num_sent, 1);
            }
        }
        // No body on our packet, just send the headers.
        else {
            if header_size < MIN_PAYLOAD_SIZE {
                let padding_bytes = MIN_PAYLOAD_SIZE - header_size;
                let padding_buf =
                    unsafe { &mut header_mbuf.slice_mut()[header_size..][..padding_bytes] };
                for byte in padding_buf {
                    *byte = 0;
                }
            }
            let frame_size = std::cmp::max(header_size, MIN_PAYLOAD_SIZE);
            header_mbuf.trim(header_mbuf.len() - frame_size);
            let mut header_mbuf_ptr = header_mbuf.into_raw();
            let num_sent =
                unsafe { rte_eth_tx_burst(inner.dpdk_port_id, 0, &mut header_mbuf_ptr, 1) };
            assert_eq!(num_sent, 1);
        }
    }

    fn receive(&self) -> ArrayVec<DPDKBuf, RECEIVE_BATCH_SIZE> {
        let inner = self.inner.borrow_mut();
        let mut out = ArrayVec::new();

        let mut packets: [*mut rte_mbuf; RECEIVE_BATCH_SIZE] = unsafe { mem::zeroed() };
        let nb_rx = unsafe {
            #[cfg(feature = "profiler")]
            timer!("catnip_libos::receive::rte_eth_rx_burst");

            rte_eth_rx_burst(
                inner.dpdk_port_id,
                0,
                packets.as_mut_ptr(),
                RECEIVE_BATCH_SIZE as u16,
            )
        };
        assert!(nb_rx as usize <= RECEIVE_BATCH_SIZE);

        {
            #[cfg(feature = "profiler")]
            timer!("catnip_libos:receive::for");
            for &packet in &packets[..nb_rx as usize] {
                let mbuf = Mbuf {
                    ptr: packet,
                    mm: inner.memory_manager.clone(),
                };
                out.push(DPDKBuf::Managed(mbuf));
            }
        }

        out
    }

    fn local_link_addr(&self) -> MacAddress {
        self.inner.borrow().link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.inner.borrow().ipv4_addr.clone()
    }

    fn tcp_options(&self) -> TcpConfig {
        self.inner.borrow().tcp_options.clone()
    }

    fn udp_options(&self) -> UdpConfig {
        self.inner.borrow().udp_options.clone()
    }

    fn arp_options(&self) -> ArpConfig {
        self.inner.borrow().arp_options.clone()
    }
}

impl SchedulerRuntime for DPDKRuntime {
    type WaitFuture = WaitFuture<TimerRc>;

    fn advance_clock(&self, now: Instant) {
        self.inner.borrow_mut().timer.0.advance_clock(now);
    }

    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let self_ = self.inner.borrow_mut();
        let now = self_.timer.0.now();
        self_
            .timer
            .0
            .wait_until(self_.timer.clone(), now + duration)
    }

    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        let self_ = self.inner.borrow_mut();
        self_.timer.0.wait_until(self_.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.inner.borrow().timer.0.now()
    }

    fn spawn<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        self.scheduler.insert(future)
    }

    fn schedule<F: SchedulerFuture>(&self, future: F) -> SchedulerHandle {
        self.scheduler.insert(future)
    }

    fn get_handle(&self, key: u64) -> Option<SchedulerHandle> {
        self.scheduler.from_raw_handle(key)
    }

    fn take(&self, handle: SchedulerHandle) -> Box<dyn SchedulerFuture> {
        self.scheduler.take(handle)
    }

    fn poll(&self) {
        self.scheduler.poll()
    }
}

impl UtilsRuntime for DPDKRuntime {
    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        let mut self_ = self.inner.borrow_mut();
        self_.rng.gen()
    }

    fn rng_shuffle<T>(&self, slice: &mut [T]) {
        let mut inner = self.inner.borrow_mut();
        slice.shuffle(&mut inner.rng);
    }
}

impl Runtime for DPDKRuntime {}
