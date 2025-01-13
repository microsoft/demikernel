// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::{
        async_queue::{AsyncQueue, SharedAsyncQueue},
        async_value::SharedAsyncValue,
    },
    expect_some,
    inetstack::{
        config::TcpConfig,
        consts::{FALLBACK_MSS, MAX_HEADER_SIZE, MAX_WINDOW_SCALE},
        protocols::{
            layer3::SharedLayer3Endpoint,
            layer4::tcp::{
                established::{
                    congestion_control::{self, CongestionControl},
                    SharedEstablishedSocket,
                },
                header::{TcpHeader, TcpOptions2},
                isn_generator::IsnGenerator,
                SeqNumber,
            },
        },
    },
    runtime::{
        conditional_yield_with_timeout, fail::Fail, memory::DemiBuffer, network::socket::option::TcpSocketOptions,
        SharedDemiRuntime, SharedObject,
    },
};
use ::futures::FutureExt;
use ::libc::{EBADMSG, ETIMEDOUT};
use ::std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddrV4},
    ops::{Deref, DerefMut},
    time::Duration,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// States of a passive socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// The socket is listening for new connections.
    Listening,
    /// The socket is closed.
    Closed,
}

pub struct PassiveSocket {
    // TCP Connection State.
    state: SharedAsyncValue<State>,
    connections: HashMap<SocketAddrV4, SharedAsyncQueue<(Ipv4Addr, TcpHeader, DemiBuffer)>>,
    ready: AsyncQueue<(SocketAddrV4, Result<SharedEstablishedSocket, Fail>)>,
    max_backlog: usize,
    isn_generator: IsnGenerator,
    local: SocketAddrV4,
    runtime: SharedDemiRuntime,
    layer3_endpoint: SharedLayer3Endpoint,
    tcp_config: TcpConfig,
    // We do not use these right now, but will in the future.
    socket_options: TcpSocketOptions,
}

#[derive(Clone)]
pub struct SharedPassiveSocket(SharedObject<PassiveSocket>);

//======================================================================================================================
// Associated Function
//======================================================================================================================

impl SharedPassiveSocket {
    pub fn new(
        local: SocketAddrV4,
        max_backlog: usize,
        runtime: SharedDemiRuntime,
        layer3_endpoint: SharedLayer3Endpoint,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
        nonce: u32,
    ) -> Result<Self, Fail> {
        Ok(Self(SharedObject::<PassiveSocket>::new(PassiveSocket {
            state: SharedAsyncValue::new(State::Listening),
            connections: HashMap::<SocketAddrV4, SharedAsyncQueue<(Ipv4Addr, TcpHeader, DemiBuffer)>>::new(),
            ready: AsyncQueue::<(SocketAddrV4, Result<SharedEstablishedSocket, Fail>)>::default(),
            max_backlog,
            isn_generator: IsnGenerator::new(nonce),
            local,
            runtime,
            layer3_endpoint,
            tcp_config,
            socket_options: default_socket_options,
        })))
    }

    /// Returns the address that the socket is bound to.
    pub fn endpoint(&self) -> SocketAddrV4 {
        self.local
    }

    /// Accept a new connection by fetching one from the queue of requests, blocking if there are no new requests.
    pub async fn do_accept(&mut self) -> Result<SharedEstablishedSocket, Fail> {
        let (_, new_socket) = self.ready.pop(None).await?;
        new_socket
    }

    // Closes the target socket.
    pub fn close(&mut self) -> Result<(), Fail> {
        self.state.set(State::Closed);
        Ok(())
    }

    pub fn receive(&mut self, ipv4_addr: Ipv4Addr, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        let remote: SocketAddrV4 = SocketAddrV4::new(ipv4_addr, tcp_hdr.src_port);

        // See if this packet is for an ongoing connection set up.
        if let Some(recv_queue) = self.connections.get_mut(&remote) {
            // Packet is either for an inflight request or established connection.
            recv_queue.push((ipv4_addr, tcp_hdr, buf));
            return;
        }

        // See if this packet is for an already established but not accepted socket.
        if let Some((_, socket)) = self.ready.get_values().find(|(addr, _)| *addr == remote) {
            if let Ok(socket) = socket {
                socket.clone().receive(tcp_hdr, buf);
            }
            return;
        }

        // Otherwise if not a SYN, then this packet is not for a new connection and we throw it away.
        if !tcp_hdr.syn || tcp_hdr.ack || tcp_hdr.rst {
            let cause: String = format!(
                "invalid TCP flags (syn={}, ack={}, rst={})",
                tcp_hdr.syn, tcp_hdr.ack, tcp_hdr.rst
            );
            warn!("poll(): {}", cause);
            self.send_rst(&remote, tcp_hdr);
            return;
        }

        // Check if this SYN segment carries any data.
        if !buf.is_empty() {
            // RFC 793 allows connections to be established with data-carrying segments, but we do not support this.
            // We simply drop the data and and proceed with the three-way handshake protocol, on the hope that the
            // remote will retransmit the data after the connection is established.
            // See: https://datatracker.ietf.org/doc/html/rfc793#section-3.4 fo more details.
            warn!("Received SYN with data (len={})", buf.len());
            // TODO: https://github.com/microsoft/demikernel/issues/1115
        }

        // Start a new connection.
        self.handle_new_syn(remote, tcp_hdr);
    }

    fn handle_new_syn(&mut self, remote: SocketAddrV4, tcp_hdr: TcpHeader) {
        debug!("Received SYN: {:?}", tcp_hdr);
        let inflight_len: usize = self.connections.len();
        // Check backlog. Since we might receive data even on connections that have completed their handshake, all
        // ready sockets are also in the inflight table.
        if inflight_len >= self.max_backlog {
            let cause: String = format!(
                "backlog full (inflight={}, ready={}, backlog={})",
                inflight_len,
                self.ready.len(),
                self.max_backlog
            );
            warn!("handle_new_syn(): {}", cause);
            self.send_rst(&remote, tcp_hdr);
            return;
        }

        // Send SYN+ACK.
        let local: SocketAddrV4 = self.local.clone();
        let local_isn = self.isn_generator.generate(&local, &remote);
        let remote_isn = tcp_hdr.seq_num;

        // Allocate a new coroutine to send the SYN+ACK and retry if necessary.
        let recv_queue: SharedAsyncQueue<(Ipv4Addr, TcpHeader, DemiBuffer)> =
            SharedAsyncQueue::<(Ipv4Addr, TcpHeader, DemiBuffer)>::default();
        let future = self
            .clone()
            .send_syn_ack_and_wait_for_ack(remote, remote_isn, local_isn, tcp_hdr, recv_queue.clone())
            .fuse();
        match self
            .runtime
            .insert_coroutine("bgc::inetstack::tcp::passiveopen::background", None, Box::pin(future))
        {
            Ok(qt) => qt,
            Err(e) => {
                let cause = "Could not allocate coroutine for passive open";
                error!("{}: {:?}", cause, e);
                return;
            },
        };
        // TODO: Clean up the connections table once we have merged all of the routing tables into one.
        self.connections.insert(remote, recv_queue);
    }

    /// Sends a RST segment to `remote`.
    fn send_rst(&mut self, remote: &SocketAddrV4, tcp_hdr: TcpHeader) {
        debug!("send_rst(): sending RST to {:?}", remote);

        // If this is an inactive socket, then generate a RST segment.
        // Generate the RST segment according to the ACK field.
        // If the incoming segment has an ACK field, the reset takes its
        // sequence number from the ACK field of the segment, otherwise the
        // reset has sequence number zero and the ACK field is set to the sum
        // of the sequence number and segment length of the incoming segment.
        // Reference: https://datatracker.ietf.org/doc/html/rfc793#section-3.4
        let (seq_num, ack_num): (SeqNumber, Option<SeqNumber>) = if tcp_hdr.ack {
            (tcp_hdr.ack_num, Some(tcp_hdr.ack_num + SeqNumber::from(1)))
        } else {
            (
                SeqNumber::from(0),
                Some(tcp_hdr.seq_num + SeqNumber::from(tcp_hdr.compute_size() as u32)),
            )
        };

        // Create a RST segment.
        let dst_ipv4_addr: Ipv4Addr = remote.ip().clone();
        let mut tcp_hdr: TcpHeader = TcpHeader::new(self.local.port(), remote.port());
        tcp_hdr.rst = true;
        tcp_hdr.seq_num = seq_num;
        if let Some(ack_num) = ack_num {
            tcp_hdr.ack = true;
            tcp_hdr.ack_num = ack_num;
        }

        // Add headers in reverse.
        let mut pkt: DemiBuffer = DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16);
        tcp_hdr.serialize_and_attach(
            &mut pkt,
            self.local.ip(),
            remote.ip(),
            self.tcp_config.get_rx_checksum_offload(),
        );

        // Pass on to send through the L2 layer.
        if let Err(e) = self.layer3_endpoint.transmit_tcp_packet_nonblocking(dst_ipv4_addr, pkt) {
            warn!("Could not send RST: {:?}", e);
        }
    }

    async fn send_syn_ack_and_wait_for_ack(
        mut self,
        remote: SocketAddrV4,
        remote_isn: SeqNumber,
        local_isn: SeqNumber,
        tcp_hdr: TcpHeader,
        recv_queue: SharedAsyncQueue<(Ipv4Addr, TcpHeader, DemiBuffer)>,
    ) {
        // Set up new inflight accept connection.
        let mut remote_window_scale = None;
        let mut mss = FALLBACK_MSS;
        for option in tcp_hdr.iter_options() {
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

        let mut handshake_retries: usize = self.tcp_config.get_handshake_retries();
        let handshake_timeout: Duration = self.tcp_config.get_handshake_timeout();

        loop {
            // Send the SYN + ACK.
            if let Err(e) = self.send_syn_ack(local_isn, remote_isn, remote).await {
                self.complete_handshake(remote, Err(e));
                return;
            }

            // Start ack timer.

            // Wait for ACK in response.
            let ack = self.clone().wait_for_ack(
                recv_queue.clone(),
                remote,
                local_isn,
                remote_isn,
                tcp_hdr.window_size,
                remote_window_scale,
                mss,
            );

            // Either we get an ack or a timeout.
            match conditional_yield_with_timeout(ack, handshake_timeout).await {
                // Got an ack
                Ok(result) => {
                    self.complete_handshake(remote, result);
                    return;
                },
                Err(Fail { errno, cause: _ }) if errno == ETIMEDOUT => {
                    if handshake_retries > 0 {
                        handshake_retries = handshake_retries - 1;
                        continue;
                    } else {
                        self.ready
                            .push((remote, Err(Fail::new(ETIMEDOUT, "handshake timeout"))));
                        return;
                    }
                },
                Err(e) => {
                    self.complete_handshake(remote, Err(e));
                    return;
                },
            }
        }
    }

    async fn send_syn_ack(
        &mut self,
        local_isn: SeqNumber,
        remote_isn: SeqNumber,
        remote: SocketAddrV4,
    ) -> Result<(), Fail> {
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
        let dst_ipv4_addr: Ipv4Addr = remote.ip().clone();
        let mut pkt: DemiBuffer = DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16);
        tcp_hdr.serialize_and_attach(
            &mut pkt,
            self.local.ip(),
            remote.ip(),
            self.tcp_config.get_rx_checksum_offload(),
        );
        self.layer3_endpoint
            .transmit_tcp_packet_blocking(dst_ipv4_addr, pkt)
            .await
    }

    async fn wait_for_ack(
        self,
        mut recv_queue: SharedAsyncQueue<(Ipv4Addr, TcpHeader, DemiBuffer)>,
        remote: SocketAddrV4,
        local_isn: SeqNumber,
        remote_isn: SeqNumber,
        remote_window_size_bytes: u16,
        remote_window_scale_bits: Option<u8>,
        mss: usize,
    ) -> Result<SharedEstablishedSocket, Fail> {
        let (tcp_hdr, buf): (TcpHeader, DemiBuffer) = loop {
            match recv_queue.pop(None).await? {
                // We expect to get a SYN+ACK with the initial seq number plus 1.
                (_, tcp_hdr, buf) if tcp_hdr.ack && tcp_hdr.ack_num == local_isn + SeqNumber::from(1) => {
                    debug!("Received ACK: {:?}", tcp_hdr);
                    break (tcp_hdr, buf);
                },
                // We got an ACK but not for the right sequence number.
                (_, tcp_hdr, _) if tcp_hdr.ack => {
                    let cause = "invalid SYN+ACK seq num";
                    warn!("{}: {:?}", cause, tcp_hdr);
                    return Err(Fail::new(EBADMSG, &cause));
                },
                // We got a duplicate SYN, so ignore it.
                (_, tcp_hdr, _) if tcp_hdr.syn && tcp_hdr.ack_num == local_isn => {
                    debug!("Received duplicate SYN: {:?}", tcp_hdr)
                },
                // We didn't get any kind of expected packet.
                _ => return Err(Fail::new(EBADMSG, "must contain an ACK")),
            }
        };

        // Calculate the window.
        let (local_window_scale_bits, remote_window_scale_bits): (u8, u8) = match remote_window_scale_bits {
            Some(remote_window_scale) => {
                if (remote_window_scale as usize) > MAX_WINDOW_SCALE {
                    warn!(
                        "remote windows scale larger than {:?} is incorrect, so setting to {:?}. See RFC 1323.",
                        MAX_WINDOW_SCALE, MAX_WINDOW_SCALE
                    );
                    (self.tcp_config.get_window_scale() as u8, MAX_WINDOW_SCALE as u8)
                } else {
                    (self.tcp_config.get_window_scale() as u8, remote_window_scale)
                }
            },
            None => (0, 0),
        };

        // Expect is safe here because the window size is a 16-bit unsigned integer and MAX_WINDOW_SCALE is 14, so it is impossible to overflow the 32-bit
        debug_assert!((remote_window_scale_bits as usize) <= MAX_WINDOW_SCALE);
        let remote_window_size_bytes: u32 = expect_some!(
            (remote_window_size_bytes as u32).checked_shl(remote_window_scale_bits as u32),
            "Window size overflow"
        );
        // Expect is safe here because the receive window size is a 16-bit unsigned integer and MAX_WINDOW_SCALE is 14,
        // so it is impossible to overflow the 32-bit unsigned int.
        debug_assert!((local_window_scale_bits as usize) <= MAX_WINDOW_SCALE);
        let local_window_size_bytes: u32 = expect_some!(
            (self.tcp_config.get_receive_window_size() as u32).checked_shl(local_window_scale_bits as u32),
            "Window size overflow"
        );
        info!(
            "Window sizes: local {} bytes, remote {} bytes",
            local_window_size_bytes, remote_window_size_bytes
        );
        info!(
            "Window scale: local {}, remote {}",
            local_window_scale_bits, remote_window_scale_bits
        );

        // Check if there is data and if so, pass it along to the established header.
        let data_with_ack: Option<(TcpHeader, DemiBuffer)> = if buf.is_empty() { None } else { Some((tcp_hdr, buf)) };
        let new_socket: SharedEstablishedSocket = SharedEstablishedSocket::new(
            self.local,
            remote,
            self.runtime.clone(),
            self.layer3_endpoint.clone(),
            data_with_ack,
            self.tcp_config.clone(),
            self.socket_options,
            remote_isn + SeqNumber::from(1),
            self.tcp_config.get_ack_delay_timeout(),
            local_window_size_bytes,
            local_window_scale_bits,
            local_isn + SeqNumber::from(1),
            remote_window_size_bytes,
            remote_window_scale_bits,
            mss,
            congestion_control::None::new,
            None,
        )?;

        Ok(new_socket)
    }

    fn complete_handshake(&mut self, remote: SocketAddrV4, result: Result<SharedEstablishedSocket, Fail>) {
        warn!("completing handshake");
        self.connections.remove(&remote);
        self.ready.push((remote, result));
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedPassiveSocket {
    type Target = PassiveSocket;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedPassiveSocket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
