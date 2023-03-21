// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
            constants::FALLBACK_MSS,
            established::{
                congestion_control::{
                    self,
                    CongestionControl,
                },
                ControlBlock,
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
    ECONNREFUSED,
    ETIMEDOUT,
};
use ::std::{
    cell::RefCell,
    convert::TryInto,
    future::Future,
    net::SocketAddrV4,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
};

struct ConnectResult {
    waker: Option<Waker>,
    result: Option<Result<ControlBlock, Fail>>,
}

pub struct ActiveOpenSocket {
    local_isn: SeqNumber,

    local: SocketAddrV4,
    remote: SocketAddrV4,

    rt: Rc<dyn NetworkRuntime>,
    scheduler: Scheduler,
    clock: TimerRc,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,
    arp: ArpPeer,

    #[allow(unused)]
    handle: SchedulerHandle,
    result: Rc<RefCell<ConnectResult>>,
}

impl ActiveOpenSocket {
    pub fn new(
        scheduler: Scheduler,
        local_isn: SeqNumber,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        rt: Rc<dyn NetworkRuntime>,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        clock: TimerRc,
        arp: ArpPeer,
    ) -> Self {
        let result = ConnectResult {
            waker: None,
            result: None,
        };
        let result = Rc::new(RefCell::new(result));

        let future = Self::background(
            local_isn,
            local,
            remote,
            rt.clone(),
            clock.clone(),
            local_link_addr,
            tcp_config.clone(),
            arp.clone(),
            result.clone(),
        );
        let task: BackgroundTask =
            BackgroundTask::new(String::from("Inetstack::TCP::activeopen::background"), Box::pin(future));

        let handle: SchedulerHandle = match scheduler.insert(task) {
            Some(handle) => handle,
            None => panic!("failed to insert task in the scheduler"),
        };

        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).
        Self {
            local_isn,
            local,
            remote,
            rt,
            scheduler: scheduler.clone(),
            clock,
            local_link_addr,
            tcp_config,
            arp,
            handle,
            result,
        }
    }

    pub fn poll_result(&mut self, context: &mut Context) -> Poll<Result<ControlBlock, Fail>> {
        let mut r = self.result.borrow_mut();
        match r.result.take() {
            None => {
                r.waker.replace(context.waker().clone());
                Poll::Pending
            },
            Some(r) => Poll::Ready(r),
        }
    }

    fn set_result(&mut self, result: Result<ControlBlock, Fail>) {
        let mut r = self.result.borrow_mut();
        if let Some(w) = r.waker.take() {
            w.wake()
        }
        r.result.replace(result);
    }

    pub fn receive(&mut self, header: &TcpHeader) {
        let expected_seq = self.local_isn + SeqNumber::from(1);

        // Bail if we didn't receive a ACK packet with the right sequence number.
        if !(header.ack && header.ack_num == expected_seq) {
            return;
        }

        // Check if our peer is refusing our connection request.
        if header.rst {
            self.set_result(Err(Fail::new(ECONNREFUSED, "connection refused")));
            return;
        }

        // Bail if we didn't receive a SYN packet.
        if !header.syn {
            return;
        }

        debug!("Received SYN+ACK: {:?}", header);

        // Acknowledge the SYN+ACK segment.
        let remote_link_addr = match self.arp.try_query(self.remote.ip().clone()) {
            Some(r) => r,
            None => panic!("TODO: Clean up ARP query control flow"),
        };
        let remote_seq_num = header.seq_num + SeqNumber::from(1);

        let mut tcp_hdr = TcpHeader::new(self.local.port(), self.remote.port());
        tcp_hdr.ack = true;
        tcp_hdr.ack_num = remote_seq_num;
        tcp_hdr.window_size = self.tcp_config.get_receive_window_size();
        tcp_hdr.seq_num = self.local_isn + SeqNumber::from(1);
        debug!("Sending ACK: {:?}", tcp_hdr);

        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ipv4_hdr: Ipv4Header::new(self.local.ip().clone(), self.remote.ip().clone(), IpProtocol::TCP),
            tcp_hdr,
            data: None,
            tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
        };
        self.rt.transmit(Box::new(segment));

        let mut remote_window_scale = None;
        let mut mss = FALLBACK_MSS;
        for option in header.iter_options() {
            match option {
                TcpOptions2::WindowScale(w) => {
                    info!("Received window scale: {}", w);
                    remote_window_scale = Some(*w);
                },
                TcpOptions2::MaximumSegmentSize(m) => {
                    info!("Received advertised MSS: {}", m);
                    mss = *m as usize;
                },
                _ => continue,
            }
        }

        let (local_window_scale, remote_window_scale) = match remote_window_scale {
            Some(w) => (self.tcp_config.get_window_scale() as u32, w),
            None => (0, 0),
        };

        // TODO(RFC1323): Clamp the scale to 14 instead of panicking.
        assert!(local_window_scale <= 14 && remote_window_scale <= 14);

        let rx_window_size: u32 = (self.tcp_config.get_receive_window_size())
            .checked_shl(local_window_scale as u32)
            .expect("TODO: Window size overflow")
            .try_into()
            .expect("TODO: Window size overflow");

        let tx_window_size: u32 = (header.window_size)
            .checked_shl(remote_window_scale as u32)
            .expect("TODO: Window size overflow")
            .try_into()
            .expect("TODO: Window size overflow");

        info!("Window sizes: local {}, remote {}", rx_window_size, tx_window_size);
        info!(
            "Window scale: local {}, remote {}",
            local_window_scale, remote_window_scale
        );

        let cb = ControlBlock::new(
            self.local,
            self.remote,
            self.rt.clone(),
            self.scheduler.clone(),
            self.clock.clone(),
            self.local_link_addr,
            self.tcp_config.clone(),
            self.arp.clone(),
            remote_seq_num,
            self.tcp_config.get_ack_delay_timeout(),
            rx_window_size,
            local_window_scale,
            expected_seq,
            tx_window_size,
            remote_window_scale,
            mss,
            congestion_control::None::new,
            None,
        );
        self.set_result(Ok(cb));
    }

    fn background(
        local_isn: SeqNumber,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        rt: Rc<dyn NetworkRuntime>,
        clock: TimerRc,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        result: Rc<RefCell<ConnectResult>>,
    ) -> impl Future<Output = ()> {
        let handshake_retries: usize = tcp_config.get_handshake_retries();
        let handshake_timeout = tcp_config.get_handshake_timeout();

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
                tcp_hdr.window_size = tcp_config.get_receive_window_size();

                let mss = tcp_config.get_advertised_mss() as u16;
                tcp_hdr.push_option(TcpOptions2::MaximumSegmentSize(mss));
                info!("Advertising MSS: {}", mss);

                tcp_hdr.push_option(TcpOptions2::WindowScale(tcp_config.get_window_scale()));
                info!("Advertising window scale: {}", tcp_config.get_window_scale());

                debug!("Sending SYN {:?}", tcp_hdr);
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
            let mut r = result.borrow_mut();
            if let Some(w) = r.waker.take() {
                w.wake()
            }
            r.result.replace(Err(Fail::new(ETIMEDOUT, "handshake timeout")));
        }
    }

    /// Returns the addresses of the two ends of this connection.
    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.local, self.remote)
    }
}
