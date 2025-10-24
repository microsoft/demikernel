// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Exports
//======================================================================================================================

pub mod congestion_control;
mod congestion_control_state;
mod connection_management_state;
pub mod ctrlblk;
mod flow_control_state;
mod ordered_delivery_state;
mod rto;

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
                    congestion_control_state::CongestionControlState,
                    connection_management_state::ConnectionManagementState,
                    ctrlblk::{ControlBlock, State},
                    flow_control_state::FlowControlState,
                    ordered_delivery_state::OrderedDeliveryState,
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

        let delivery = OrderedDeliveryState::new(
            sender_seq_no,
            receiver_seq_no,
            receiver_seq_no,
            ack_delay_timeout_secs,
            receiver_window_size_bytes,
            receiver_window_scale_bits,
        );

        let flow_control = FlowControlState::new(
            sender_seq_no,
            receiver_seq_no,
            sender_window_size_bytes,
            sender_window_scale_bits,
            sender_mss,
        );
        let connection_management = ConnectionManagementState::new(local, remote, tcp_config, default_socket_options);

        // Initialize congestion control state, which starts with default RtoCalculator
        let congestion_control_algorithm = cc_constructor(sender_mss, sender_seq_no, congestion_control_options);
        let congestion_control = CongestionControlState::new(congestion_control_algorithm);
        let cb = ControlBlock::new(connection_management, delivery, flow_control, congestion_control);
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

    pub fn receive(&mut self, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        debug!(
            "{:?} Connection Receiving {} bytes + {:?}",
            self.control_block.connection_management.state,
            buf.len(),
            tcp_hdr,
        );

        let now = self.runtime.now();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        OrderedDeliveryState::receive(&mut self.control_block, &mut layer3_endpoint, tcp_hdr, buf, now);
    }

    // This coroutine runs the close protocol.
    pub async fn close(&mut self) -> Result<(), Fail> {
        // Assert we are in a valid state and move to new state.
        match self.control_block.connection_management.state {
            State::Established => self.local_close().await,
            State::CloseWait => self.remote_already_closed().await,
            _ => {
                let cause = "socket is already closing";
                error!("close(): {}", cause);
                Err(Fail::new(libc::EBADF, cause))
            },
        }
    }

    async fn local_close(&mut self) -> Result<(), Fail> {
        // 1. Start close protocol by setting state and sending FIN.
        self.control_block.connection_management.state = State::FinWait1;

        // 2. Wait for FIN and FIN ack.
        let mut me2 = self.clone();
        let mut me3 = self.clone();
        let wait_for_fin = pin!(me3.control_block.delivery.wait_for_fin().fuse());
        let mut runtime = self.runtime.clone();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        let push_fin_and_wait_for_ack = pin!(OrderedDeliveryState::push(
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
        debug_assert_eq!(self.control_block.connection_management.state, State::TimeWait);
        trace!(
            "socket options: {:?}",
            self.control_block.connection_management.socket_options.get_linger()
        );
        let timeout = self
            .control_block
            .connection_management
            .socket_options
            .get_linger()
            .unwrap_or(MSL * 2);
        yield_with_timeout(timeout).await;
        self.control_block.connection_management.state = State::Closed;
        Ok(())
    }

    async fn remote_already_closed(&mut self) -> Result<(), Fail> {
        // 0. Move state forward
        self.control_block.connection_management.state = State::LastAck;
        // 1. Send FIN and wait for ack before closing.
        let mut runtime = self.runtime.clone();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        OrderedDeliveryState::push(
            &mut self.control_block,
            &mut layer3_endpoint,
            &mut runtime,
            ArrayVec::new(),
        )
        .await?;
        debug_assert_eq!(self.control_block.connection_management.state, State::Closed);

        Ok(())
    }

    pub async fn push(&mut self, bufs: ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>) -> Result<(), Fail> {
        let mut runtime = self.runtime.clone();
        let mut layer3_endpoint = self.layer3_endpoint.clone();
        OrderedDeliveryState::push(&mut self.control_block, &mut layer3_endpoint, &mut runtime, bufs).await
    }

    pub async fn pop(&mut self, size: Option<usize>) -> Result<ArrayVec<DemiBuffer, MAX_BATCH_SIZE_NUM_PACKETS>, Fail> {
        self.control_block.delivery.pop(size).await
    }

    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (
            self.control_block.connection_management.local,
            self.control_block.connection_management.remote,
        )
    }

    async fn background(self) {
        let mut me = self.clone();
        let acknowledger = async_timer!("tcp::established::background::acknowledger", async {
            let mut layer3_endpoint = me.layer3_endpoint.clone();
            OrderedDeliveryState::acknowledger(&mut me.control_block, &mut layer3_endpoint).await
        })
        .fuse();
        pin_mut!(acknowledger);

        let mut me2 = self.clone();
        let retransmitter = async_timer!("tcp::established::background::retransmitter", async {
            let mut layer3_endpoint = me2.layer3_endpoint.clone();
            let mut runtime = me2.runtime.clone();
            OrderedDeliveryState::background_retransmitter(&mut me2.control_block, &mut layer3_endpoint, &mut runtime)
                .await
        })
        .fuse();
        pin_mut!(retransmitter);

        let mut me3 = self.clone();
        let sender = async_timer!("tcp::established::background::sender", async {
            let mut layer3_endpoint = me3.layer3_endpoint.clone();
            let mut runtime = me3.runtime.clone();
            OrderedDeliveryState::background_sender(&mut me3.control_block, &mut layer3_endpoint, &mut runtime).await
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
