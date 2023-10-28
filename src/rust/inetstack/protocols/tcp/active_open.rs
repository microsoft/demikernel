// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_value::AsyncValue,
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
                SharedControlBlock,
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
        timer::SharedTimer,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
    scheduler::{
        TaskHandle,
        Yielder,
    },
};
use ::libc::{
    ECONNREFUSED,
    ETIMEDOUT,
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

pub struct ActiveOpenSocket<const N: usize> {
    local_isn: SeqNumber,

    local: SocketAddrV4,
    remote: SocketAddrV4,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    clock: SharedTimer,
    local_link_addr: MacAddress,
    tcp_config: TcpConfig,
    arp: SharedArpPeer<N>,
    result: AsyncValue<Result<SharedControlBlock<N>, Fail>>,
    handle: Option<TaskHandle>,
}

#[derive(Clone)]
pub struct SharedActiveOpenSocket<const N: usize>(SharedObject<ActiveOpenSocket<N>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<const N: usize> SharedActiveOpenSocket<N> {
    pub fn new(
        local_isn: SeqNumber,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        mut runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        clock: SharedTimer,
        arp: SharedArpPeer<N>,
    ) -> Result<Self, Fail> {
        let mut me: Self = Self(SharedObject::<ActiveOpenSocket<N>>::new(ActiveOpenSocket::<N> {
            local_isn,
            local,
            remote,
            runtime: runtime.clone(),
            transport,
            clock,
            local_link_addr,
            tcp_config,
            arp,
            result: AsyncValue::<Result<SharedControlBlock<N>, Fail>>::default(),
            handle: None,
        }));

        let handle: TaskHandle = runtime.insert_background_coroutine(
            "Inetstack::TCP::activeopen::background",
            Box::pin(me.clone().background()),
        )?;
        me.handle = Some(handle);
        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).
        Ok(me)
    }

    pub fn receive(&mut self, header: &TcpHeader) {
        let expected_seq = self.local_isn + SeqNumber::from(1);

        // Bail if we didn't receive a ACK packet with the right sequence number.
        if !(header.ack && header.ack_num == expected_seq) {
            return;
        }

        // Check if our peer is refusing our connection request.
        if header.rst {
            self.result.set(Err(Fail::new(ECONNREFUSED, "connection refused")));
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

        let cb = SharedControlBlock::new(
            self.local,
            self.remote,
            self.runtime.clone(),
            self.transport.clone(),
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
        self.result.set(Ok(cb));
        let handle: TaskHandle = self.handle.take().expect("We should have allocated a background task");
        if let Err(e) = self.runtime.remove_background_coroutine(&handle) {
            panic!("Failed to remove active open coroutine (error={:?}", e);
        }
    }

    async fn background(mut self) {
        let handshake_retries: usize = self.tcp_config.get_handshake_retries();
        let handshake_timeout = self.tcp_config.get_handshake_timeout();
        for _ in 0..handshake_retries {
            let remote_link_addr = match self.clone().arp.query(self.remote.ip().clone()).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("ARP query failed: {:?}", e);
                    continue;
                },
            };

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
            self.transport.transmit(Box::new(segment));
            let clock_ref: SharedTimer = self.clock.clone();
            let yielder: Yielder = Yielder::new();
            if let Err(e) = clock_ref.wait(handshake_timeout, yielder).await {
                self.result.set(Err(e));
                return;
            }
        }
        self.result.set(Err(Fail::new(ETIMEDOUT, "handshake timeout")));
    }

    pub async fn get_result(mut self, yielder: Yielder) -> Result<SharedControlBlock<N>, Fail> {
        self.result.get(yielder).await?
    }

    /// Returns the addresses of the two ends of this connection.
    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.local, self.remote)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Deref for SharedActiveOpenSocket<N> {
    type Target = ActiveOpenSocket<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedActiveOpenSocket<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
