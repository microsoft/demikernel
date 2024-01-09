// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    collections::async_queue::{
        AsyncQueue,
        SharedAsyncQueue,
    },
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
            Yielder,
            YielderHandle,
        },
        QDesc,
        SharedDemiRuntime,
        SharedObject,
    },
    QToken,
};
use ::futures::{
    channel::mpsc,
    pin_mut,
    select_biased,
    FutureExt,
};
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

pub struct PassiveSocket<N: NetworkRuntime> {
    connections: HashMap<SocketAddrV4, SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>>,
    recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
    ready: AsyncQueue<Result<EstablishedSocket<N>, Fail>>,
    max_backlog: usize,
    isn_generator: IsnGenerator,
    local: SocketAddrV4,
    runtime: SharedDemiRuntime,
    transport: N,
    tcp_config: TcpConfig,
    local_link_addr: MacAddress,
    arp: SharedArpPeer<N>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    yielder_handle: YielderHandle,
    background_task_qt: Option<QToken>,
}

#[derive(Clone)]
pub struct SharedPassiveSocket<N: NetworkRuntime>(SharedObject<PassiveSocket<N>>);

//======================================================================================================================
// Associated Function
//======================================================================================================================

impl<N: NetworkRuntime> SharedPassiveSocket<N> {
    pub fn new(
        local: SocketAddrV4,
        max_backlog: usize,
        mut runtime: SharedDemiRuntime,
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        transport: N,
        tcp_config: TcpConfig,
        local_link_addr: MacAddress,
        arp: SharedArpPeer<N>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        nonce: u32,
    ) -> Result<Self, Fail> {
        let yielder: Yielder = Yielder::new();
        let mut me: Self = Self(SharedObject::<PassiveSocket<N>>::new(PassiveSocket {
            connections: HashMap::<SocketAddrV4, SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>>::new(),
            recv_queue,
            ready: AsyncQueue::<Result<EstablishedSocket<N>, Fail>>::default(),
            max_backlog,
            isn_generator: IsnGenerator::new(nonce),
            local,
            local_link_addr,
            runtime: runtime.clone(),
            transport,
            tcp_config,
            arp,
            dead_socket_tx,
            yielder_handle: yielder.get_handle(),
            background_task_qt: None,
        }));
        let qt: QToken = runtime
            .insert_background_coroutine("passive_listening::poll", Box::pin(me.clone().poll(yielder).fuse()))?;
        me.background_task_qt = Some(qt);
        Ok(me)
    }

    /// Returns the address that the socket is bound to.
    pub fn endpoint(&self) -> SocketAddrV4 {
        self.local
    }

    /// Accept a new connection by fetching one from the queue of requests, blocking if there are no new requests.
    pub async fn do_accept(&mut self, yielder: Yielder) -> Result<EstablishedSocket<N>, Fail> {
        self.ready.pop(&yielder).await?
    }

    async fn poll(mut self, yielder: Yielder) {
        loop {
            let (ipv4_hdr, tcp_hdr, buf) = match self.recv_queue.pop(&yielder).await {
                Ok(result) => result,
                Err(_) => break,
            };
            let remote: SocketAddrV4 = SocketAddrV4::new(ipv4_hdr.get_src_addr(), tcp_hdr.src_port);
            if let Some(recv_queue) = self.connections.get_mut(&remote) {
                // Packet is either for an inflight request or established connection.
                recv_queue.push((ipv4_hdr, tcp_hdr, buf));
                continue;
            }

            // If not a SYN, then this packet is not for a new connection and we throw it away.
            if !tcp_hdr.syn || tcp_hdr.ack || tcp_hdr.rst {
                let cause: String = format!(
                    "invalid TCP flags (syn={}, ack={}, rst={})",
                    tcp_hdr.syn, tcp_hdr.ack, tcp_hdr.rst
                );
                warn!("poll(): {}", cause);
                self.send_rst(&remote, tcp_hdr);
                continue;
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
    }

    fn handle_new_syn(&mut self, remote: SocketAddrV4, tcp_hdr: TcpHeader) {
        debug!("Received SYN: {:?}", tcp_hdr);
        let inflight_len: usize = self.connections.len();
        if inflight_len + self.ready.len() >= self.max_backlog {
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
        let yielder: Yielder = Yielder::new();
        let recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)> =
            SharedAsyncQueue::<(Ipv4Header, TcpHeader, DemiBuffer)>::default();
        let ack_queue: SharedAsyncQueue<usize> = SharedAsyncQueue::<usize>::default();
        let future = self
            .clone()
            .send_syn_ack_and_wait_for_ack(
                remote,
                remote_isn,
                local_isn,
                tcp_hdr,
                recv_queue.clone(),
                ack_queue,
                yielder,
            )
            .fuse();
        match self
            .runtime
            .insert_background_coroutine("Inetstack::TCP::passiveopen::background", Box::pin(future))
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

        // Query link address for destination.
        let dst_link_addr: MacAddress = match self.arp.try_query(remote.ip().clone()) {
            Some(link_addr) => link_addr,
            None => {
                // ARP query is unlikely to fail, but if it does, don't send the RST segment,
                // and return an error to server side.
                let cause: String = format!("missing ARP entry (remote={})", remote.ip());
                error!("send_rst(): {}", &cause);
                return;
            },
        };

        // Create a RST segment.
        let segment: TcpSegment = {
            let mut tcp_hdr: TcpHeader = TcpHeader::new(self.local.port(), remote.port());
            tcp_hdr.rst = true;
            tcp_hdr.seq_num = seq_num;
            if let Some(ack_num) = ack_num {
                tcp_hdr.ack = true;
                tcp_hdr.ack_num = ack_num;
            }
            TcpSegment {
                ethernet2_hdr: Ethernet2Header::new(dst_link_addr, self.local_link_addr, EtherType2::Ipv4),
                ipv4_hdr: Ipv4Header::new(self.local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
                tcp_hdr,
                data: None,
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            }
        };

        // Send it.
        let pkt: Box<TcpSegment> = Box::new(segment);
        self.transport.transmit(pkt);
    }

    async fn send_syn_ack_and_wait_for_ack(
        mut self,
        remote: SocketAddrV4,
        remote_isn: SeqNumber,
        local_isn: SeqNumber,
        tcp_hdr: TcpHeader,
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        ack_queue: SharedAsyncQueue<usize>,
        yielder: Yielder,
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
                self.ready.push(Err(e));
                return;
            }

            // Start ack timer.
            let timeout_yielder: Yielder = Yielder::new();
            let timeout = self
                .runtime
                .get_timer()
                .wait(handshake_timeout, &timeout_yielder)
                .fuse();
            // Wait for ACK in response.
            let ack = self
                .clone()
                .wait_for_ack(
                    recv_queue.clone(),
                    ack_queue.clone(),
                    remote,
                    local_isn,
                    remote_isn,
                    tcp_hdr.window_size,
                    remote_window_scale,
                    mss,
                    &yielder,
                )
                .fuse();
            // Pin futures.
            pin_mut!(timeout);
            pin_mut!(ack);

            // Either we get an ack or a timeout.
            select_biased! {
                r = ack => match r {
                    // Got an ack
                    Ok(socket) => {
                        self.ready.push(Ok(socket));
                        return;
                    },
                    Err(e) => {
                        self.ready.push(Err(e));
                        return;
                    }
                },
                r = timeout => match r {
                    Ok(()) if handshake_retries > 0  => {
                        handshake_retries = handshake_retries - 1;
                        continue;
                    },
                    Ok(()) => {
                        self.ready.push(Err(Fail::new(ETIMEDOUT, "handshake timeout")));
                        return;
                    },
                    Err(e) => {
                        self.ready.push(Err(e));
                        return;
                    }
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
        let remote_link_addr = self.arp.query(remote.ip().clone(), &Yielder::new()).await?;
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
        Ok(())
    }

    async fn wait_for_ack(
        self,
        mut recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        ack_queue: SharedAsyncQueue<usize>,
        remote: SocketAddrV4,
        local_isn: SeqNumber,
        remote_isn: SeqNumber,
        header_window_size: u16,
        remote_window_scale: Option<u8>,
        mss: usize,
        yielder: &Yielder,
    ) -> Result<EstablishedSocket<N>, Fail> {
        let (ipv4_hdr, tcp_hdr, buf) = recv_queue.pop(&yielder).await?;
        debug!("Received ACK: {:?}", tcp_hdr);

        // Check the ack sequence number.
        if tcp_hdr.ack_num != local_isn + SeqNumber::from(1) {
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

        // If there is data with the SYN+ACK, deliver it.
        if !buf.is_empty() {
            recv_queue.push((ipv4_hdr, tcp_hdr, buf));
        }

        let new_socket: EstablishedSocket<N> = EstablishedSocket::<N>::new(
            self.local,
            remote,
            self.runtime.clone(),
            self.transport.clone(),
            recv_queue.clone(),
            ack_queue,
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

        Ok(new_socket)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<N: NetworkRuntime> Deref for SharedPassiveSocket<N> {
    type Target = PassiveSocket<N>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<N: NetworkRuntime> DerefMut for SharedPassiveSocket<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<N: NetworkRuntime> Drop for PassiveSocket<N> {
    fn drop(&mut self) {
        if let Some(qt) = self.background_task_qt.take() {
            self.yielder_handle
                .wake_with(Err(Fail::new(libc::ECANCELED, "Socket is closing")));
            if let Err(e) = self.runtime.remove_background_coroutine(qt) {
                warn!("Could not remove background coroutine: {:?}", e);
            }
        }
    }
}
