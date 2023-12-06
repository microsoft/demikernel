// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::AsyncQueue,
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
                congestion_control,
                congestion_control::CongestionControl,
                EstablishedSocket,
            },
            isn_generator::IsnGenerator,
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
        scheduler::{
            TaskHandle,
            Yielder,
            YielderHandle,
        },
        timer::SharedTimer,
        QDesc,
        SharedBox,
        SharedDemiRuntime,
        SharedObject,
    },
};
use ::core::panic;
use ::futures::channel::mpsc;
use ::libc::{
    EBADMSG,
    ETIMEDOUT,
};
use ::std::{
    collections::HashMap,
    convert::TryInto,
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

struct InflightAccept {
    local_isn: SeqNumber,
    remote_isn: SeqNumber,
    header_window_size: u16,
    remote_window_scale: Option<u8>,
    mss: usize,
    handle: TaskHandle,
    yielder_handle: YielderHandle,
}

pub struct PassiveSocket<const N: usize> {
    inflight: HashMap<SocketAddrV4, InflightAccept>,
    ready: AsyncQueue<Result<EstablishedSocket<N>, Fail>>,
    max_backlog: usize,
    isn_generator: IsnGenerator,
    local: SocketAddrV4,
    runtime: SharedDemiRuntime,
    transport: SharedBox<dyn NetworkRuntime<N>>,
    tcp_config: TcpConfig,
    local_link_addr: MacAddress,
    arp: SharedArpPeer<N>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
}

#[derive(Clone)]
pub struct SharedPassiveSocket<const N: usize>(SharedObject<PassiveSocket<N>>);

//======================================================================================================================
// Associated Function
//======================================================================================================================

impl<const N: usize> SharedPassiveSocket<N> {
    pub fn new(
        local: SocketAddrV4,
        max_backlog: usize,
        runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime<N>>,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        arp: SharedArpPeer<N>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        nonce: u32,
    ) -> Self {
        Self(SharedObject::<PassiveSocket<N>>::new(PassiveSocket::<N> {
            inflight: HashMap::new(),
            ready: AsyncQueue::<Result<EstablishedSocket<N>, Fail>>::default(),
            max_backlog,
            isn_generator: IsnGenerator::new(nonce),
            local,
            local_link_addr,
            runtime,
            transport,
            tcp_config,
            arp,
            dead_socket_tx,
        }))
    }

    /// Returns the address that the socket is bound to.
    pub fn endpoint(&self) -> SocketAddrV4 {
        self.local
    }

    /// Accept a new connection by fetching one from the queue of requests, blocking if there are no new requests.
    pub async fn do_accept(&mut self, yielder: Yielder) -> Result<EstablishedSocket<N>, Fail> {
        self.ready.pop(&yielder).await?
    }

    /// Receive and direct new connection requests and ACKs.
    pub fn receive(&mut self, ip_header: &Ipv4Header, header: TcpHeader, buf: DemiBuffer) -> Result<(), Fail> {
        let remote = SocketAddrV4::new(ip_header.get_src_addr(), header.src_port);
        // If the packet is for an inflight connection, route it there.
        if let Some(inflight) = self.inflight.remove(&remote) {
            // If ack for inflight connection request, remove from inflight table and handle. If error, we do not put
            // the inflight request back but drop it.
            // FIXME: https://github.com/microsoft/demikernel/issues/1054
            if header.ack {
                return self.handle_ack(inflight, remote, header, buf);
            } else {
                return Err(Fail::new(EBADMSG, "expecting ACK"));
            }
        } else {
            // Check whether this packet is for a connection that we have finished accepting.
            for result in self.ready.get_mut_values() {
                match result {
                    // We've finished establishing the connection, so just deliver the packet.
                    Ok(ref mut socket) if socket.endpoints().1 == remote => {
                        socket.receive(header, buf);
                        return Ok(());
                    },
                    _ => continue,
                }
            }
        }

        // If not a SYN, then this packet is not for a new connection and we throw it away.
        if !header.syn || header.ack || header.rst {
            return Err(Fail::new(EBADMSG, "invalid flags"));
        }
        // Start a new connection.
        self.handle_syn(remote, header)
    }

    fn handle_syn(&mut self, remote: SocketAddrV4, header: TcpHeader) -> Result<(), Fail> {
        debug!("Received SYN: {:?}", header);
        let inflight_len: usize = self.inflight.len();
        if inflight_len + self.ready.len() >= self.max_backlog {
            let cause: String = format!(
                "backlog full (inflight={}, ready={}, backlog={})",
                inflight_len,
                self.ready.len(),
                self.max_backlog
            );
            error!("receive(): {:?}", &cause);
            return Err(Fail::new(libc::ECONNREFUSED, &cause));
        }

        // Send SYN+ACK.
        let local: SocketAddrV4 = self.local.clone();
        let local_isn = self.isn_generator.generate(&local, &remote);
        let remote_isn = header.seq_num;

        // Allocate a new coroutine to send the SYN+ACK and retry if necessary.
        let yielder: Yielder = Yielder::new();
        let yielder_handle: YielderHandle = yielder.get_handle();
        let future = self.clone().send_syn_ack(remote, remote_isn, local_isn, yielder);
        let handle: TaskHandle = self
            .runtime
            .insert_background_coroutine("Inetstack::TCP::passiveopen::background", Box::pin(future))?;

        // Set up new inflight accept connection.
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
            yielder_handle,
        };
        self.inflight.insert(remote, accept);
        Ok(())
    }

    fn handle_ack(
        &mut self,
        mut inflight: InflightAccept,
        remote: SocketAddrV4,
        header: TcpHeader,
        buf: DemiBuffer,
    ) -> Result<(), Fail> {
        debug!("Received ACK: {:?}", header);
        // Grab values from inflight accept.
        let InflightAccept {
            local_isn,
            remote_isn,
            header_window_size,
            remote_window_scale,
            mss,
            ..
        } = inflight;

        // Check the ack sequence number.
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

        let mut new_socket: EstablishedSocket<N> = EstablishedSocket::<N>::new(
            self.local,
            remote,
            self.runtime.clone(),
            self.transport.clone(),
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
            self.dead_socket_tx.clone(),
        )?;

        // If there is data with the SYN+ACK, deliver it.
        if !buf.is_empty() {
            new_socket.receive(header, buf);
        }

        // Remove SYN+ACK coroutine. Setting the yielder handle will keep it from being woken in the future.
        inflight.yielder_handle.wake_with(Ok(()));
        if let Err(e) = self.runtime.remove_background_coroutine(&inflight.handle) {
            panic!("Failed to remove inflight accept (error={:?})", e);
        }

        self.ready.push(Ok(new_socket));
        Ok(())
    }

    async fn send_syn_ack(
        mut self,
        remote: SocketAddrV4,
        remote_isn: SeqNumber,
        local_isn: SeqNumber,
        yielder: Yielder,
    ) {
        let handshake_retries: usize = self.tcp_config.get_handshake_retries();
        let handshake_timeout: Duration = self.tcp_config.get_handshake_timeout();

        for _ in 0..handshake_retries {
            let remote_link_addr = match self.arp.query(remote.ip().clone(), &Yielder::new()).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("ARP query failed: {:?}", e);
                    continue;
                },
            };
            let mut tcp_hdr = TcpHeader::new(self.local.port(), remote.port());
            tcp_hdr.syn = true;
            tcp_hdr.seq_num = local_isn;
            tcp_hdr.ack = true;
            tcp_hdr.ack_num = remote_isn + SeqNumber::from(1);
            tcp_hdr.window_size = self.tcp_config.get_receive_window_size();

            let mss = self.tcp_config.get_advertised_mss() as u16;
            tcp_hdr.push_option(TcpOptions2::MaximumSegmentSize(mss));
            info!("Advertising MSS: {}", mss);

            tcp_hdr.push_option(TcpOptions2::WindowScale(self.tcp_config.get_window_scale()));
            info!("Advertising window scale: {}", self.tcp_config.get_window_scale());

            debug!("Sending SYN+ACK: {:?}", tcp_hdr);
            let segment = TcpSegment {
                ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
                ipv4_hdr: Ipv4Header::new(self.local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
                tcp_hdr,
                data: None,
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            };
            self.transport.transmit(Box::new(segment));
            let clock_ref: SharedTimer = self.runtime.get_timer();
            if let Err(e) = clock_ref.wait(handshake_timeout, &yielder).await {
                self.ready.push(Err(e));
                return;
            }
        }
        self.ready.push(Err(Fail::new(ETIMEDOUT, "handshake timeout")));
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Deref for SharedPassiveSocket<N> {
    type Target = PassiveSocket<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<const N: usize> DerefMut for SharedPassiveSocket<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
