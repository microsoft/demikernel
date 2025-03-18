// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::{async_queue::SharedAsyncQueue, async_value::SharedAsyncValue},
    expect_some,
    inetstack::{
        config::TcpConfig,
        consts::{FALLBACK_MSS, MAX_HEADER_SIZE, MAX_WINDOW_SCALE},
        protocols::{
            layer3::NetworkLayer,
            layer4::tcp::{
                established::{
                    congestion_control::{self, CongestionControl},
                    SharedEstablishedSocket,
                },
                header::{TcpHeader, TcpOptions2},
                SeqNumber,
            },
        },
    },
    runtime::{
        fail::Fail, memory::DemiBuffer, network::socket::option::TcpSocketOptions, SharedDemiRuntime, SharedObject,
    },
};
use ::futures::{select_biased, FutureExt};
use ::std::{
    net::{Ipv4Addr, SocketAddrV4},
    ops::{Deref, DerefMut},
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// States of a connecting socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// The socket is listening for new connections.
    Connecting,
    /// The socket is closed.
    Closed,
}

pub struct ActiveOpenSocket<T: NetworkLayer> {
    local_isn: SeqNumber,
    local: SocketAddrV4,
    remote: SocketAddrV4,
    runtime: SharedDemiRuntime,
    layer3_endpoint: T,
    recv_queue: SharedAsyncQueue<(Ipv4Addr, TcpHeader, T::FlowRecord, DemiBuffer)>,
    tcp_config: TcpConfig,
    socket_options: TcpSocketOptions,
    state: SharedAsyncValue<State>,
}

#[derive(Clone)]
pub struct SharedActiveOpenSocket<T: NetworkLayer>(SharedObject<ActiveOpenSocket<T>>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<T: NetworkLayer> SharedActiveOpenSocket<T> {
    pub fn new(
        local_isn: SeqNumber,
        local: SocketAddrV4,
        remote: SocketAddrV4,
        runtime: SharedDemiRuntime,
        layer3_endpoint: T,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
    ) -> Result<Self, Fail> {
        // TODO: Add fast path here when remote is already in the ARP cache (and subtract one retry).

        Ok(Self(SharedObject::<ActiveOpenSocket<T>>::new(ActiveOpenSocket::<T> {
            local_isn,
            local,
            remote,
            runtime: runtime.clone(),
            layer3_endpoint,
            recv_queue: SharedAsyncQueue::default(),
            tcp_config,
            socket_options: default_socket_options,
            state: SharedAsyncValue::new(State::Connecting),
        })))
    }

    fn process_ack(
        &mut self,
        header: TcpHeader,
        flow_record: T::FlowRecord,
    ) -> Result<SharedEstablishedSocket<T>, Fail> {
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

        let remote_seq_num = header.seq_num + SeqNumber::from(1);

        let mut tcp_hdr = TcpHeader::new(self.local.port(), self.remote.port());
        tcp_hdr.ack = true;
        tcp_hdr.ack_num = remote_seq_num;
        tcp_hdr.window_size = self.tcp_config.get_receive_window_size();
        tcp_hdr.seq_num = self.local_isn + SeqNumber::from(1);
        debug!("Sending ACK: {:?}", tcp_hdr);

        let dst_ipv4_addr: Ipv4Addr = self.remote.ip().clone();
        let mut pkt: DemiBuffer = DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16);
        tcp_hdr.serialize_and_attach(
            &mut pkt,
            self.local.ip(),
            self.remote.ip(),
            self.tcp_config.get_rx_checksum_offload(),
        );
        let flow_state: T::FlowState = {
            let mut flow_state = T::FlowState::default();
            self.layer3_endpoint.update_flow_state(&mut flow_state, flow_record);
            flow_state
        };
        self.layer3_endpoint
            .transmit_tcp_packet_nonblocking(dst_ipv4_addr, &flow_state, pkt)?;

        let mut remote_window_scale_bits = None;
        let mut mss = FALLBACK_MSS;
        for option in header.iter_options() {
            match option {
                TcpOptions2::WindowScale(w) => {
                    info!("Received window scale: {}", w);
                    remote_window_scale_bits = Some(*w);
                },
                TcpOptions2::MaximumSegmentSize(m) => {
                    info!("Received advertised MSS: {}", m);
                    mss = *m as usize;
                },
                _ => continue,
            }
        }

        let (local_window_scale_bits, remote_window_scale_bits): (u8, u8) = match remote_window_scale_bits {
            Some(remote_window_scale_bits) => {
                let remote: u8 = if remote_window_scale_bits as usize > MAX_WINDOW_SCALE {
                    warn!(
                        "remote windows scale larger than {:?} is incorrect, so setting to {:?}. See RFC 1323.",
                        MAX_WINDOW_SCALE, MAX_WINDOW_SCALE
                    );
                    MAX_WINDOW_SCALE as u8
                } else {
                    remote_window_scale_bits
                };
                (self.tcp_config.get_window_scale() as u8, remote)
            },
            None => (0, 0),
        };

        // Expect is safe here because the receive window size is a 16-bit unsigned integer and MAX_WINDOW_SCALE is 14,
        // so it is impossible to overflow the 32-bit unsigned int.
        debug_assert!((local_window_scale_bits as usize) <= MAX_WINDOW_SCALE);
        let rx_window_size_bytes: u32 = expect_some!(
            (self.tcp_config.get_receive_window_size() as u32).checked_shl(local_window_scale_bits as u32),
            "Window size overflow"
        );
        // Expect is safe here because the window size is a 16-bit unsigned integer and MAX_WINDOW_SCALE is 14, so it is impossible to overflow the 32-bit
        debug_assert!((remote_window_scale_bits as usize) <= MAX_WINDOW_SCALE);
        let tx_window_size_bytes: u32 = expect_some!(
            (header.window_size as u32).checked_shl(remote_window_scale_bits as u32),
            "Window size overflow"
        );

        info!(
            "Window sizes: local {} bytes, remote {} bytes",
            rx_window_size_bytes, tx_window_size_bytes
        );
        info!(
            "Window scale: local {}, remote {}",
            local_window_scale_bits, remote_window_scale_bits
        );

        Ok(SharedEstablishedSocket::new(
            self.local,
            self.remote,
            self.runtime.clone(),
            self.layer3_endpoint.clone(),
            flow_state,
            None,
            self.tcp_config.clone(),
            self.socket_options,
            remote_seq_num,
            self.tcp_config.get_ack_delay_timeout(),
            rx_window_size_bytes,
            local_window_scale_bits,
            expected_seq,
            tx_window_size_bytes,
            remote_window_scale_bits,
            mss,
            congestion_control::None::new,
            None,
        )?)
    }

    pub async fn connect(mut self) -> Result<SharedEstablishedSocket<T>, Fail> {
        // Start connection handshake.
        let handshake_retries: usize = self.tcp_config.get_handshake_retries();
        let handshake_timeout = self.tcp_config.get_handshake_timeout();

        // Try to connect.
        for _ in 0..handshake_retries {
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
            let dst_ipv4_addr: Ipv4Addr = self.remote.ip().clone();
            let mut pkt: DemiBuffer = DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16);
            tcp_hdr.serialize_and_attach(
                &mut pkt,
                self.local.ip(),
                self.remote.ip(),
                self.tcp_config.get_rx_checksum_offload(),
            );
            // Send SYN.
            let flow_state: T::FlowState = T::FlowState::default();
            if let Err(e) = self
                .layer3_endpoint
                .transmit_tcp_packet_blocking(dst_ipv4_addr, &flow_state, pkt)
                .await
            {
                warn!("Could not send SYN: {:?}", e);
                continue;
            }

            // Wait for either a response or timeout.
            let mut recv_queue: SharedAsyncQueue<(Ipv4Addr, TcpHeader, T::FlowRecord, DemiBuffer)> =
                self.recv_queue.clone();
            let mut state: SharedAsyncValue<State> = self.state.clone();
            select_biased! {
            r = state.wait_for_change(None).fuse() => if let Ok(r) = r {
                if r == State::Closed {
                    let cause: &str = "Closing socket while connecting";
                    warn!("{}", cause);
                    return Err(Fail::new(libc::ECONNABORTED, &cause));
                }
            },
            r = recv_queue.pop(Some(handshake_timeout)).fuse() => match r {
                Ok((_, header, flow_record, _)) => match self.process_ack(header, flow_record) {
                        Ok(socket) => return Ok(socket),
                        Err(Fail { errno, cause: _ }) if errno == libc::EAGAIN => continue,
                        Err(e) => return Err(e),
                    },
                    Err(Fail { errno, cause: _ }) if errno == libc::ETIMEDOUT => continue,
                    Err(_) => {
                        unreachable!(
                            "either the ack deadline changed or the deadline passed, no other errors are possible!"
                        )
                    },
                }
            }
        }

        let cause: String = format!("connection handshake timed out");
        error!("connect(): {}", cause);
        Err(Fail::new(libc::ECONNREFUSED, &cause))
    }

    pub fn close(&mut self) {
        self.state.set(State::Closed);
    }

    /// Returns the addresses of the two ends of this connection.
    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.local, self.remote)
    }

    pub fn receive(&mut self, ipv4_addr: Ipv4Addr, tcp_hdr: TcpHeader, flow_record: T::FlowRecord, buf: DemiBuffer) {
        self.recv_queue.push((ipv4_addr, tcp_hdr, flow_record, buf))
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<T: NetworkLayer> Deref for SharedActiveOpenSocket<T> {
    type Target = ActiveOpenSocket<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: NetworkLayer> DerefMut for SharedActiveOpenSocket<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
