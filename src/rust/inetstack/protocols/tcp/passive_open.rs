// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::{
    constants::FALLBACK_MSS,
    established::ControlBlock,
    isn_generator::IsnGenerator,
};
use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            established::{
                congestion_control,
                congestion_control::CongestionControl,
            },
            segment::{
                TcpHeader,
                TcpOptions2,
                TcpSegment,
            },
            SeqNumber,
        },
    },
    runtime::{
        fail::Fail,
        network::{
            config::TcpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        queue::BackgroundTask,
        timer::TimerRc,
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
    },
};
use ::libc::{
    EBADMSG,
    ECONNREFUSED,
    ETIMEDOUT,
};
use ::std::{
    cell::RefCell,
    collections::{
        HashMap,
        HashSet,
        VecDeque,
    },
    convert::TryInto,
    future::Future,
    net::SocketAddrV4,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    time::Duration,
};

struct InflightAccept {
    local_isn: SeqNumber,
    remote_isn: SeqNumber,
    header_window_size: u16,
    remote_window_scale: Option<u8>,
    mss: usize,

    #[allow(unused)]
    handle: SchedulerHandle,
}

struct ReadySockets {
    ready: VecDeque<Result<ControlBlock, Fail>>,
    endpoints: HashSet<SocketAddrV4>,
    waker: Option<Waker>,
}

impl ReadySockets {
    fn push_ok(&mut self, cb: ControlBlock) {
        assert!(self.endpoints.insert(cb.get_remote()));
        self.ready.push_back(Ok(cb));
        if let Some(w) = self.waker.take() {
            w.wake()
        }
    }

    fn push_err(&mut self, err: Fail) {
        self.ready.push_back(Err(err));
        if let Some(w) = self.waker.take() {
            w.wake()
        }
    }

    fn poll(&mut self, ctx: &mut Context) -> Poll<Result<ControlBlock, Fail>> {
        let r = match self.ready.pop_front() {
            Some(r) => r,
            None => {
                self.waker.replace(ctx.waker().clone());
                return Poll::Pending;
            },
        };
        if let Ok(ref cb) = r {
            assert!(self.endpoints.remove(&cb.get_remote()));
        }
        Poll::Ready(r)
    }

    fn len(&self) -> usize {
        self.ready.len()
    }
}

pub struct PassiveSocket {
    inflight: HashMap<SocketAddrV4, InflightAccept>,
    ready: Rc<RefCell<ReadySockets>>,

    max_backlog: usize,
    isn_generator: IsnGenerator,

    local: SocketAddrV4,
    rt: Rc<dyn NetworkRuntime>,
    scheduler: Scheduler,
    clock: TimerRc,
    tcp_config: TcpConfig,
    local_link_addr: MacAddress,
    arp: ArpPeer,
}

impl PassiveSocket {
    pub fn new(
        local: SocketAddrV4,
        max_backlog: usize,
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        clock: TimerRc,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        arp: ArpPeer,
        nonce: u32,
    ) -> Self {
        let ready = ReadySockets {
            ready: VecDeque::new(),
            endpoints: HashSet::new(),
            waker: None,
        };
        let ready = Rc::new(RefCell::new(ready));
        Self {
            inflight: HashMap::new(),
            ready,
            max_backlog,
            isn_generator: IsnGenerator::new(nonce),
            local,
            local_link_addr,
            rt,
            scheduler,
            clock,
            tcp_config,
            arp,
        }
    }

    /// Returns the address that the socket is bound to.
    pub fn endpoint(&self) -> SocketAddrV4 {
        self.local
    }

    pub fn poll_accept(&mut self, ctx: &mut Context) -> Poll<Result<ControlBlock, Fail>> {
        self.ready.borrow_mut().poll(ctx)
    }

    pub fn receive(&mut self, ip_header: &Ipv4Header, header: &TcpHeader) -> Result<(), Fail> {
        let remote = SocketAddrV4::new(ip_header.get_src_addr(), header.src_port);
        if self.ready.borrow().endpoints.contains(&remote) {
            // TODO: What should we do if a packet shows up for a connection that hasn't been `accept`ed yet?
            return Ok(());
        }
        let inflight_len = self.inflight.len();

        // If the packet is for an inflight connection, route it there.
        if self.inflight.contains_key(&remote) {
            if !header.ack {
                return Err(Fail::new(EBADMSG, "expeting ACK"));
            }
            debug!("Received ACK: {:?}", header);
            let &InflightAccept {
                local_isn,
                remote_isn,
                header_window_size,
                remote_window_scale,
                mss,
                ..
            } = self.inflight.get(&remote).unwrap();
            if header.ack_num != local_isn + SeqNumber::from(1) {
                return Err(Fail::new(EBADMSG, "invalid SYN+ACK seq num"));
            }

            let (local_window_scale, remote_window_scale) = match remote_window_scale {
                Some(w) => (self.tcp_config.get_window_scale() as u32, w),
                None => (0, 0),
            };
            let remote_window_size = (header_window_size)
                .checked_shl(remote_window_scale as u32)
                .expect("TODO: Window size overflow")
                .try_into()
                .expect("TODO: Window size overflow");
            let local_window_size = (self.tcp_config.get_receive_window_size() as u32)
                .checked_shl(local_window_scale as u32)
                .expect("TODO: Window size overflow");
            info!(
                "Window sizes: local {}, remote {}",
                local_window_size, remote_window_size
            );
            info!(
                "Window scale: local {}, remote {}",
                local_window_scale, remote_window_scale
            );

            self.inflight.remove(&remote);
            let cb = ControlBlock::new(
                self.local,
                remote,
                self.rt.clone(),
                self.scheduler.clone(),
                self.clock.clone(),
                self.local_link_addr,
                self.tcp_config.clone(),
                self.arp.clone(),
                remote_isn + SeqNumber::from(1),
                self.tcp_config.get_ack_delay_timeout(),
                local_window_size,
                local_window_scale,
                local_isn + SeqNumber::from(1),
                remote_window_size,
                remote_window_scale,
                mss,
                congestion_control::None::new,
                None,
            );
            self.ready.borrow_mut().push_ok(cb);
            return Ok(());
        }

        // Otherwise, start a new connection.
        if !header.syn || header.ack || header.rst {
            return Err(Fail::new(EBADMSG, "invalid flags"));
        }
        debug!("Received SYN: {:?}", header);
        if inflight_len + self.ready.borrow().len() >= self.max_backlog {
            // TODO: Should we send a RST here?
            return Err(Fail::new(ECONNREFUSED, "connection refused"));
        }
        let local_isn = self.isn_generator.generate(&self.local, &remote);
        let remote_isn = header.seq_num;
        let future = Self::background(
            local_isn,
            remote_isn,
            self.local,
            remote,
            self.rt.clone(),
            self.clock.clone(),
            self.tcp_config.clone(),
            self.local_link_addr,
            self.arp.clone(),
            self.ready.clone(),
        );
        let task: BackgroundTask = BackgroundTask::new(
            String::from("Inetstack::TCP::passiveopen::background"),
            Box::pin(future),
        );
        let handle: SchedulerHandle = match self.scheduler.insert(task) {
            Some(handle) => handle,
            None => panic!("failed to insert task in the scheduler"),
        };

        let mut remote_window_scale = None;
        let mut mss = FALLBACK_MSS;
        for option in header.iter_options() {
            match option {
                TcpOptions2::WindowScale(w) => {
                    info!("Received window scale: {:?}", w);
                    remote_window_scale = Some(*w);
                },
                TcpOptions2::MaximumSegmentSize(m) => {
                    info!("Received advertised MSS: {}", m);
                    mss = *m as usize;
                },
                _ => continue,
            }
        }
        let accept = InflightAccept {
            local_isn,
            remote_isn,
            header_window_size: header.window_size,
            remote_window_scale,
            mss,
            handle,
        };
        self.inflight.insert(remote, accept);
        Ok(())
    }

    fn background(
        local_isn: SeqNumber,
        remote_isn: SeqNumber,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        rt: Rc<dyn NetworkRuntime>,
        clock: TimerRc,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        arp: ArpPeer,
        ready: Rc<RefCell<ReadySockets>>,
    ) -> impl Future<Output = ()> {
        let handshake_retries: usize = tcp_config.get_handshake_retries();
        let handshake_timeout: Duration = tcp_config.get_handshake_timeout();

        async move {
            for _ in 0..handshake_retries {
                let remote_link_addr = match arp.query(remote.ip().clone()).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("ARP query failed: {:?}", e);
                        continue;
                    },
                };
                let mut tcp_hdr = TcpHeader::new(local.port(), remote.port());
                tcp_hdr.syn = true;
                tcp_hdr.seq_num = local_isn;
                tcp_hdr.ack = true;
                tcp_hdr.ack_num = remote_isn + SeqNumber::from(1);
                tcp_hdr.window_size = tcp_config.get_receive_window_size();

                let mss = tcp_config.get_advertised_mss() as u16;
                tcp_hdr.push_option(TcpOptions2::MaximumSegmentSize(mss));
                info!("Advertising MSS: {}", mss);

                tcp_hdr.push_option(TcpOptions2::WindowScale(tcp_config.get_window_scale()));
                info!("Advertising window scale: {}", tcp_config.get_window_scale());

                debug!("Sending SYN+ACK: {:?}", tcp_hdr);
                let segment = TcpSegment {
                    ethernet2_hdr: Ethernet2Header::new(remote_link_addr, local_link_addr, EtherType2::Ipv4),
                    ipv4_hdr: Ipv4Header::new(local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
                    tcp_hdr,
                    data: None,
                    tx_checksum_offload: tcp_config.get_rx_checksum_offload(),
                };
                rt.transmit(Box::new(segment));
                clock.wait(clock.clone(), handshake_timeout).await;
            }
            ready.borrow_mut().push_err(Fail::new(ETIMEDOUT, "handshake timeout"));
        }
    }
}
