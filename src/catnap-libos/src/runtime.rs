use anyhow::Error;
use arrayvec::ArrayVec;
use catnip::{
    collections::bytes::{
        Bytes,
        BytesMut,
    },
    futures::FutureOperation,
    interop::{
        dmtr_sgarray_t,
        dmtr_sgaseg_t,
    },
    protocols::{
        arp::ArpConfig,
        ethernet2::{
            Ethernet2Header,
            MacAddress,
        },
        tcp,
        udp::UdpConfig,
    },
    runtime::{
        PacketBuf,
        Runtime,
        RECEIVE_BATCH_SIZE,
    },
    timer::{
        Timer,
        TimerRc,
        WaitFuture,
    },
};
use catwalk::{
    Scheduler,
    SchedulerHandle,
};
use futures::{
    Future,
    FutureExt,
};
use libc;
use rand::{
    distributions::Standard,
    prelude::Distribution,
    rngs::SmallRng,
    seq::SliceRandom,
    Rng,
    SeedableRng,
};
use socket2::{
    Domain,
    SockAddr,
    Socket,
    Type,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    convert::TryInto,
    fs,
    mem::{
        self,
        MaybeUninit,
    },
    net::Ipv4Addr,
    ptr,
    rc::Rc,
    slice,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Constants & Structures
//==============================================================================

// ETH_P_ALL must be converted to big-endian short but (due to a bug in Rust libc bindings) comes as an int.
const ETH_P_ALL: libc::c_ushort = (libc::ETH_P_ALL as libc::c_ushort).to_be();
enum SockAddrPurpose {
    Bind,
    Send,
}

#[derive(Clone)]
pub struct LinuxRuntime {
    inner: Rc<RefCell<Inner>>,
    scheduler: Scheduler<FutureOperation<LinuxRuntime>>,
}

pub struct Inner {
    pub timer: TimerRc,
    pub rng: SmallRng,
    pub socket: Socket,
    pub ifindex: i32,
    pub link_addr: MacAddress,
    pub ipv4_addr: Ipv4Addr,
    pub tcp_options: tcp::Options<LinuxRuntime>,
    pub arp_options: ArpConfig,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl SockAddrPurpose {
    fn protocol(&self) -> libc::c_ushort {
        match self {
            SockAddrPurpose::Bind => ETH_P_ALL,
            SockAddrPurpose::Send => 0,
        }
    }

    fn halen(&self) -> libc::c_uchar {
        match self {
            SockAddrPurpose::Bind => 0,
            SockAddrPurpose::Send => libc::ETH_ALEN.try_into().unwrap(),
        }
    }
}

impl LinuxRuntime {
    pub fn new(
        now: Instant,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
        interface_name: &str,
        arp: HashMap<Ipv4Addr, MacAddress>,
    ) -> Self {
        let arp_options: ArpConfig = ArpConfig::new(
            Duration::from_secs(600),
            Duration::from_secs(1),
            2,
            arp,
            false,
        );

        let socket = Socket::new(
            Domain::PACKET,
            Type::RAW.nonblocking(),
            Some((ETH_P_ALL as libc::c_int).into()),
        )
        .unwrap();
        let path: String = format!("/sys/class/net/{}/ifindex", interface_name);
        let ifindex: i32 = fs::read_to_string(path)
            .expect("Could not read ifindex")
            .trim()
            .parse()
            .unwrap();

        socket
            .bind(&raw_sockaddr(SockAddrPurpose::Bind, ifindex, &[0; 6]))
            .unwrap();

        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            rng: SmallRng::from_seed([0; 32]),
            socket,
            ifindex,
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

//==============================================================================
// Trait Implementations
//==============================================================================

impl Runtime for LinuxRuntime {
    type Buf = Bytes;
    type WaitFuture = WaitFuture<TimerRc>;

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
        let mut buf = BytesMut::zeroed(len as usize).unwrap();
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
        let header_size = pkt.header_size();
        let body_size = pkt.body_size();

        let mut buf = BytesMut::zeroed(header_size + body_size).unwrap();

        pkt.write_header(&mut buf[..header_size]);
        if let Some(body) = pkt.take_body() {
            buf[header_size..].copy_from_slice(&body[..]);
        }

        let buf = buf.freeze();
        let (header, _) = Ethernet2Header::parse(buf.clone()).unwrap();
        let dest_addr_arr = header.dst_addr().to_array();
        let dest_sockaddr = raw_sockaddr(
            SockAddrPurpose::Send,
            self.inner.borrow().ifindex,
            &dest_addr_arr,
        );

        self.inner
            .borrow()
            .socket
            .send_to(&buf, &dest_sockaddr)
            .unwrap();
    }

    fn receive(&self) -> ArrayVec<Bytes, RECEIVE_BATCH_SIZE> {
        // 4096B buffer size chosen arbitrarily, seems fine for now.
        // This use-case is an example for MaybeUninit in the docs
        let mut out: [MaybeUninit<u8>; 4096] =
            [unsafe { MaybeUninit::uninit().assume_init() }; 4096];
        if let Ok((bytes_read, _origin_addr)) = self.inner.borrow().socket.recv_from(&mut out[..]) {
            let mut ret = ArrayVec::new();
            unsafe {
                let out = mem::transmute::<[MaybeUninit<u8>; 4096], [u8; 4096]>(out);
                ret.push(BytesMut::from(&out[..bytes_read]).freeze());
            }
            ret
        } else {
            ArrayVec::new()
        }
    }

    fn scheduler(&self) -> &Scheduler<FutureOperation<Self>> {
        &self.scheduler
    }

    fn local_link_addr(&self) -> MacAddress {
        self.inner.borrow().link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.inner.borrow().ipv4_addr.clone()
    }

    fn tcp_options(&self) -> tcp::Options<Self> {
        self.inner.borrow().tcp_options.clone()
    }

    fn udp_options(&self) -> UdpConfig {
        UdpConfig::default()
    }

    fn arp_options(&self) -> ArpConfig {
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
            .insert(FutureOperation::Background(future.boxed_local()))
    }
}

//==============================================================================
// Helper Functions
//==============================================================================

fn raw_sockaddr(purpose: SockAddrPurpose, ifindex: i32, mac_addr: &[u8; 6]) -> SockAddr {
    let mut padded_address = [0_u8; 8];
    padded_address[..6].copy_from_slice(mac_addr);
    let sockaddr_ll = libc::sockaddr_ll {
        sll_family: libc::AF_PACKET.try_into().unwrap(),
        sll_protocol: purpose.protocol(),
        sll_ifindex: ifindex,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: purpose.halen(),
        sll_addr: padded_address,
    };

    unsafe {
        let sockaddr_ptr =
            mem::transmute::<*const libc::sockaddr_ll, *const libc::sockaddr_storage>(&sockaddr_ll);
        SockAddr::new(
            *sockaddr_ptr,
            mem::size_of::<libc::sockaddr_ll>().try_into().unwrap(),
        )
    }
}

pub fn initialize_linux(
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    interface_name: &str,
    arp_table: HashMap<Ipv4Addr, MacAddress>,
) -> Result<LinuxRuntime, Error> {
    Ok(LinuxRuntime::new(
        Instant::now(),
        local_link_addr,
        local_ipv4_addr,
        interface_name,
        arp_table,
    ))
}
