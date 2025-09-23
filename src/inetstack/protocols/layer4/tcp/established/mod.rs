// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod congestion_control;
pub mod ctrlblk;
mod receiver;
mod rto;
mod sender;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    async_timer,
    inetstack::{
        config::TcpConfig,
        consts::{MAX_BATCH_SIZE_NUM_PACKETS, MSL},
        protocols::{
            layer3::SharedLayer3Endpoint,
            layer4::tcp::{
                congestion_control::CongestionControlConstructor,
                established::{
                    ctrlblk::{ControlBlock, State},
                    receiver::Receiver,
                    sender::Sender,
                },
                header::TcpHeader,
                SeqNumber,
            },
        },
    },
    runtime::{
        fail::Fail, memory::DemiBuffer, network::socket::option::TcpSocketOptions, yield_with_timeout,
        SharedDemiRuntime, SharedObject,
    },
};
use ::arrayvec::ArrayVec;
use ::futures::{join, pin_mut, FutureExt};
use ::std::{
    net::SocketAddrV4,
    ops::{Deref, DerefMut},
    pin::pin,
    time::Duration,
};

//======================================================================================================================
// Constants
//======================================================================================================================

// The max possible window size without scaling is 64KB.
const MAX_WINDOW_SIZE_WITHOUT_SCALING: u32 = 65535;
// THe max possible window size with scaling is 1GB.
const MAX_WINDOW_SIZE_WITH_SCALING: u32 = 1073741824;

//======================================================================================================================
// Structures
//======================================================================================================================

pub struct EstablishedSocket {
    // All shared state for this established TCP connection.
    control_block: ControlBlock,
    runtime: SharedDemiRuntime,
    layer3_endpoint: SharedLayer3Endpoint,
}

#[derive(Clone)]
pub struct SharedEstablishedSocket(SharedObject<EstablishedSocket>);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl SharedEstablishedSocket {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        mut runtime: SharedDemiRuntime,
        layer3_endpoint: SharedLayer3Endpoint,
        data_from_ack: Option<(TcpHeader, DemiBuffer)>,
        tcp_config: TcpConfig,
        default_socket_options: TcpSocketOptions,
        receiver_seq_no: SeqNumber,
        ack_delay_timeout_secs: Duration,
        receiver_window_size_bytes: u32,
        receiver_window_scale_bits: u8,
        sender_seq_no: SeqNumber,
        sender_window_size_bytes: u32,
        sender_window_scale_bits: u8,
        sender_mss: usize,
        cc_constructor: CongestionControlConstructor,
        congestion_control_options: Option<congestion_control::Options>,
    ) -> Result<Self, Fail> {
        // Check that the send window size is not too large.
        match sender_window_scale_bits {
            0 if sender_window_size_bytes > MAX_WINDOW_SIZE_WITHOUT_SCALING => {
                let cause = "Sender window too large";
                warn!(
                    "{}: scale={:?} window={:?}",
                    cause, sender_window_scale_bits, sender_window_size_bytes
                );
                return Err(Fail::new(libc::EINVAL, cause));
            },
            _ if sender_window_size_bytes > MAX_WINDOW_SIZE_WITH_SCALING => {
                let cause = "Sender window too large";
                warn!(
                    "{}: scale={:?} window={:?}",
                    cause, sender_window_scale_bits, sender_window_size_bytes
                );
                return Err(Fail::new(libc::EINVAL, cause));
            },
            _ => (),
        };

        // Check that the receive window size is not too large.
        match receiver_window_scale_bits {
            0 if receiver_window_size_bytes > MAX_WINDOW_SIZE_WITHOUT_SCALING => {
                let cause = "Receiver window too large";
                warn!(
                    "{}: scale={:?} window={:?}",
                    cause, receiver_window_scale_bits, receiver_window_size_bytes
                );
                return Err(Fail::new(libc::EINVAL, cause));
            },
            _ if receiver_window_size_bytes > MAX_WINDOW_SIZE_WITH_SCALING => {
                let cause = "Receiver window too large";
                warn!(
                    "{}: scale={:?} window={:?}",
                    cause, receiver_window_scale_bits, receiver_window_size_bytes
                );
                return Err(Fail::new(libc::EINVAL, cause));
            },
            _ => (),
        };

        let sender = Sender::new(
            sender_seq_no,
            receiver_seq_no,
            sender_window_size_bytes,
            sender_window_scale_bits,
            sender_mss,
        );
        let receiver = Receiver::new(
            receiver_seq_no,
            receiver_seq_no,
            ack_delay_timeout_secs,
            receiver_window_size_bytes,
            receiver_window_scale_bits,
        );

        let congestion_control_algorithm = cc_constructor(sender_mss, sender_seq_no, congestion_control_options);
        let cb = ControlBlock::new(
            local,
            remote,
            tcp_config,
            default_socket_options,
            sender,
            receiver,
            congestion_control_algorithm,
        );
        let mut me = Self(SharedObject::new(EstablishedSocket {
            control_block: cb,
            runtime: runtime.clone(),
            layer3_endpoint,
        }));

        // Process data carried with the response to the SYN+ACK
        if let Some((header, data)) = data_from_ack {
            me.receive(header, data);
        }
        let me2 = me.clone();
        runtime.schedule_coroutine(
            "bgc::inetstack::tcp::established::background",
            Box::pin(async move { me2.background().await }.fuse()),
        )?;
        Ok(me)
    }

    pub fn receive(&mut self, tcp_hdr: TcpHeader, buf: DemiBuffer) -> Result<(), Fail> {
        debug!(
            "{:?} Connection Receiving {} bytes + {:?}",
            self.control_block.state,
            buf.len(),
            tcp_hdr,
        );

        let now = self.runtime.now();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        Receiver::receive(&mut self.control_block, &mut layer3_endpoint, tcp_hdr, buf, now)
    }

    pub fn hard_close(&mut self) -> Result<(), Fail> {
        if self.control_block.state == State::Reset || self.control_block.state == State::Closed {
            return Ok(());
        }

        let mut layer3_endpoint = self.layer3_endpoint.clone();
        self.control_block.state = State::Reset;
        let cause = "connection reset by local host";
        Sender::send_rst_and_reset(&mut self.control_block, &mut layer3_endpoint, cause);

        // NB don't reset receiver before sending RST to avoid breaking dependencies between Sender and Receiver.
        Receiver::reset(&mut self.control_block, cause);
        Ok(())
    }

    // This coroutine runs the close protocol.
    pub async fn close(&mut self) -> Result<(), Fail> {
        match self.control_block.state {
            State::Established => self.local_close().await,
            State::CloseWait => self.remote_already_closed().await,
            State::FinWait1 | State::FinWait2 | State::Closing | State::LastAck | State::TimeWait => {
                let cause = "socket is already closing";
                error!("close(): {}", cause);
                Err(Fail::new(libc::EBADF, cause))
            },
            State::Closed => {
                let cause = "connection already closed";
                error!("close(): {}", cause);
                Err(Fail::new(libc::EBADF, cause))
            },
            State::Reset => {
                let cause = "connection reset by peer";
                Err(Fail::new(libc::ECONNRESET, cause))
            },
        }
    }

    async fn local_close(&mut self) -> Result<(), Fail> {
        // 1. Start close protocol by setting state and sending FIN.
        debug_assert!(self.control_block.state == State::Established);
        self.control_block.state = State::FinWait1;

        // 2. Wait for FIN and FIN ack.
        let mut me2 = self.clone();
        let mut me3 = self.clone();
        let wait_for_fin = pin!(me3.control_block.receiver.wait_for_fin().fuse());
        let mut runtime = self.runtime.clone();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        let push_fin_and_wait_for_ack = pin!(Sender::push(
            &mut me2.control_block,
            &mut layer3_endpoint,
            &mut runtime,
            ArrayVec::new()
        )
        .fuse());
        let (result1, result2) = join!(wait_for_fin, push_fin_and_wait_for_ack);
        result1?;
        result2?;

        // 3. TIMED_WAIT
        match self.control_block.state {
            State::TimeWait => {
                trace!("socket options: {:?}", self.control_block.socket_options.get_linger());
                let timeout = self.control_block.socket_options.get_linger().unwrap_or(MSL * 2);
                yield_with_timeout(timeout).await;
                self.control_block.state = State::Closed;
                Ok(())
            },
            State::Reset => {
                let cause = "connection reset during close handshake";
                Err(Fail::new(libc::ECONNRESET, cause))
            },
            other => {
                let cause = "unexpected state after close handshake";
                Sender::reset(&mut self.control_block, cause);
                warn!("local_close(): ended in state {:?} (treating as I/O error)", other);
                Err(Fail::new(libc::EIO, cause))
            },
        }
    }

    async fn remote_already_closed(&mut self) -> Result<(), Fail> {
        if self.control_block.state == State::Reset {
            let cause = "connection reset by peer";
            return Err(Fail::new(libc::ECONNRESET, cause));
        }

        // 0. Move state forward
        self.control_block.state = State::LastAck;

        // 1. Send FIN and wait for ack before closing.
        let mut runtime = self.runtime.clone();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        Sender::push(
            &mut self.control_block,
            &mut layer3_endpoint,
            &mut runtime,
            ArrayVec::new(),
        )
        .await?;
        debug_assert_eq!(self.control_block.state, State::Closed);

        match self.control_block.state {
            State::Closed => Ok(()),
            State::Reset => {
                let cause = "connection reset during remote_already_closed";
                Err(Fail::new(libc::ECONNRESET, cause))
            },
            other => {
                let cause = "unexpected state after remote_already_closed";
                Sender::reset(&mut self.control_block, cause);
                warn!("remote_already_closed(): ended in state {:?}", other);
                Err(Fail::new(libc::EIO, cause))
            },
        }
    }

    pub async fn push(&mut self, bufs: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>) -> Result<(), Fail> {
        let mut runtime = self.runtime.clone();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        Sender::push(&mut self.control_block, &mut layer3_endpoint, &mut runtime, bufs).await
    }

    pub async fn pop(&mut self, size: Option<usize>) -> Result<ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        Receiver::pop(&mut self.control_block, size).await
    }

    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.control_block.local, self.control_block.remote)
    }

    async fn background(self) {
        let mut me = self.clone();
        let acknowledger = async_timer!("tcp::established::background::acknowledger", async {
            let mut layer3_endpoint = me.layer3_endpoint.clone();
            Receiver::acknowledger(&mut me.control_block, &mut layer3_endpoint).await
        })
        .fuse();
        pin_mut!(acknowledger);

        let mut me2 = self.clone();
        let retransmitter = async_timer!("tcp::established::background::retransmitter", async {
            let mut layer3_endpoint = me2.layer3_endpoint.clone();
            let mut runtime = me2.runtime.clone();
            Sender::background_retransmitter(&mut me2.control_block, &mut layer3_endpoint, &mut runtime).await
        })
        .fuse();
        pin_mut!(retransmitter);

        let mut me3 = self.clone();
        let sender = async_timer!("tcp::established::background::sender", async {
            let mut layer3_endpoint = me3.layer3_endpoint.clone();
            let mut runtime = me3.runtime.clone();
            Sender::background_sender(&mut me3.control_block, &mut layer3_endpoint, &mut runtime).await
        })
        .fuse();
        pin_mut!(sender);

        let result = futures::join!(acknowledger, retransmitter, sender);
        debug!("{:?}", result);
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Deref for SharedEstablishedSocket {
    type Target = EstablishedSocket;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for SharedEstablishedSocket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
