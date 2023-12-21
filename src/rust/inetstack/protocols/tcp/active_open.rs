// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::SharedAsyncQueue,
    inetstack::protocols::{
        arp::SharedArpPeer,
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
                EstablishedSocket,
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
        memory::DemiBuffer,
        network::{
            config::TcpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        scheduler::Yielder,
        QDesc,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::futures::{
    channel::mpsc,
    future::FutureExt,
    select_biased,
};
use ::std::{
    convert::TryInto,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct ActiveOpenSocket<N: NetworkRuntime> {
    local_isn: SeqNumber,
    local: SocketAddrV4,
    remote: SocketAddrV4,
    runtime: SharedDemiRuntime,
    transport: N,
    recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
    ack_queue: SharedAsyncQueue<usize>,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,
    arp: SharedArpPeer<N>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
}

#[derive(Clone)]
pub struct SharedActiveOpenSocket<N: NetworkRuntime>(SharedObject<ActiveOpenSocket<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<N: NetworkRuntime> SharedActiveOpenSocket<N> {
    pub fn new(
        local_isn: SeqNumber,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        runtime: SharedDemiRuntime,
        transport: N,
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        ack_queue: SharedAsyncQueue<usize>,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        arp: SharedArpPeer<N>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    ) -> Result<Self, Fail> {
        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).

        Ok(Self(SharedObject::<ActiveOpenSocket<N>>::new(ActiveOpenSocket::<N> {
            local_isn,
            local,
            remote,
            runtime: runtime.clone(),
            transport,
            recv_queue,
            ack_queue,
            local_link_addr,
            tcp_config,
            arp,
            dead_socket_tx,
        })))
    }

    fn process_ack(&mut self, header: TcpHeader) -> Result<EstablishedSocket<N>, Fail> {
        let expected_seq: SeqNumber = self.local_isn + SeqNumber::from(1);

        // Bail if we didn't receive a ACK packet with the right sequence number.
        if !(header.ack && header.ack_num == expected_seq) {
            let cause: String = format!(
                "expected ack_num: {}, received ack_num: {}",
                expected_seq, header.ack_num
            );
            error!("process_ack(): {}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
        }

        // Check if our peer is refusing our connection request.
        if header.rst {
            let cause: String = format!("connection refused");
            error!("process_ack(): {}", cause);
            return Err(Fail::new(libc::ECONNREFUSED, &cause));
        }

        // Bail if we didn't receive a SYN packet.
        if !header.syn {
            let cause: String = format!("is not a syn packet");
            error!("process_ack(): {}", cause);
            return Err(Fail::new(libc::EAGAIN, &cause));
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
        self.transport.transmit(Box::new(segment));

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
        Ok(EstablishedSocket::new(
            self.local,
            self.remote,
            self.runtime.clone(),
            self.transport.clone(),
            self.recv_queue.clone(),
            self.ack_queue.clone(),
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
            self.dead_socket_tx.clone(),
        )?)
    }

    pub async fn connect(mut self, yielder: Yielder) -> Result<EstablishedSocket<N>, Fail> {
        // Start connection handshake.
        let handshake_retries: usize = self.tcp_config.get_handshake_retries();
        let handshake_timeout = self.tcp_config.get_handshake_timeout();
        for _ in 0..handshake_retries {
            // Look up remote MAC address.
            // TODO: Do we need to do this every iteration?
            let remote_link_addr = match self.clone().arp.query(self.remote.ip().clone(), &yielder).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("ARP query failed: {:?}", e);
                    continue;
                },
            };

            // Set up SYN packet.
            let mut tcp_hdr = TcpHeader::new(self.local.port(), self.remote.port());
            tcp_hdr.syn = true;
            tcp_hdr.seq_num = self.local_isn;
            tcp_hdr.window_size = self.tcp_config.get_receive_window_size();

            let mss = self.tcp_config.get_advertised_mss() as u16;
            tcp_hdr.push_option(TcpOptions2::MaximumSegmentSize(mss));
            info!("Advertising MSS: {}", mss);

            tcp_hdr.push_option(TcpOptions2::WindowScale(self.tcp_config.get_window_scale()));
            info!("Advertising window scale: {}", self.tcp_config.get_window_scale());

            debug!("Sending SYN {:?}", tcp_hdr);
            let segment = TcpSegment {
                ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
                ipv4_hdr: Ipv4Header::new(self.local.ip().clone(), self.remote.ip().clone(), IpProtocol::TCP),
                tcp_hdr,
                data: None,
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            };
            // Send SYN.
            self.transport.transmit(Box::new(segment));

            // Wait for either a response or timeout.
            let yielder2: Yielder = Yielder::new();
            let timeout_future = self.runtime.get_timer().wait(handshake_timeout, &yielder2).fuse();
            let mut me: Self = self.clone();
            let ack_future = me.recv_queue.pop(&yielder).fuse();
            futures::pin_mut!(timeout_future);
            futures::pin_mut!(ack_future);
            select_biased! {
                // If we received a response, process the response and either finish setting up the connection or try
                // again.
                result = ack_future => match result {
                    Ok((_, header, _)) => match self.process_ack(header) {
                        Ok(socket) => return Ok(socket),
                        Err(Fail{errno, cause:_}) if errno == libc::EAGAIN => continue,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                },
                // If timeout, then we try again unless this coroutine has been canceled.
                result = timeout_future => match result {
                    Ok(()) => continue,
                    Err(e) => return Err(e),
                },
            }
        }

        let cause: String = format!("connection handshake timed out");
        error!("connect(): {}", cause);
        Err(Fail::new(libc::ETIMEDOUT, &cause))
    }

    /// Returns the addresses of the two ends of this connection.
    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.local, self.remote)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<N: NetworkRuntime> Deref for SharedActiveOpenSocket<N> {
    type Target = ActiveOpenSocket<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedActiveOpenSocket<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
