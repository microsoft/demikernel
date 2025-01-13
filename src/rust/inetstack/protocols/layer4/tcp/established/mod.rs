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
        consts::MSL,
        protocols::{
            layer3::SharedLayer3Endpoint,
            layer4::tcp::{
                congestion_control::CongestionControlConstructor,
                established::{ctrlblk::ControlBlock, ctrlblk::State, receiver::Receiver, sender::Sender},
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
use ::futures::pin_mut;
use ::futures::FutureExt;
use ::std::{
    net::SocketAddrV4,
    ops::{Deref, DerefMut},
    time::Duration,
    time::Instant,
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
    cb: ControlBlock,
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
                return Err(Fail::new(libc::EINVAL, &cause));
            },
            _ if sender_window_size_bytes > MAX_WINDOW_SIZE_WITH_SCALING => {
                let cause = "Sender window too large";
                warn!(
                    "{}: scale={:?} window={:?}",
                    cause, sender_window_scale_bits, sender_window_size_bytes
                );
                return Err(Fail::new(libc::EINVAL, &cause));
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
                return Err(Fail::new(libc::EINVAL, &cause));
            },
            _ if receiver_window_size_bytes > MAX_WINDOW_SIZE_WITH_SCALING => {
                let cause = "Receiver window too large";
                warn!(
                    "{}: scale={:?} window={:?}",
                    cause, receiver_window_scale_bits, receiver_window_size_bytes
                );
                return Err(Fail::new(libc::EINVAL, &cause));
            },
            _ => (),
        };

        let sender: Sender = Sender::new(
            sender_seq_no,
            sender_window_size_bytes,
            sender_window_scale_bits,
            sender_mss,
        );
        let receiver: Receiver = Receiver::new(
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
        let mut me: Self = Self(SharedObject::new(EstablishedSocket {
            cb,
            runtime: runtime.clone(),
            layer3_endpoint,
        }));

        // Process data carried with the response to the SYN+ACK
        if let Some((header, data)) = data_from_ack {
            me.receive(header, data);
        }
        let me2: Self = me.clone();
        runtime.insert_coroutine(
            "bgc::inetstack::tcp::established::background",
            None,
            Box::pin(async move { me2.background().await }.fuse()),
        )?;
        Ok(me)
    }

    pub fn receive(&mut self, tcp_hdr: TcpHeader, buf: DemiBuffer) {
        debug!(
            "{:?} Connection Receiving {} bytes + {:?}",
            self.cb.state,
            buf.len(),
            tcp_hdr,
        );

        let now: Instant = self.runtime.get_now();
        let mut layer3_endpoint: SharedLayer3Endpoint = self.layer3_endpoint.clone();
        Receiver::receive(&mut self.cb, &mut layer3_endpoint, tcp_hdr, buf, now);
    }

    // This coroutine runs the close protocol.
    pub async fn close(&mut self) -> Result<(), Fail> {
        // Assert we are in a valid state and move to new state.
        match self.cb.state {
            State::Established => self.local_close().await,
            State::CloseWait => self.remote_already_closed().await,
            _ => {
                let cause: String = format!("socket is already closing");
                error!("close(): {}", cause);
                Err(Fail::new(libc::EBADF, &cause))
            },
        }
    }

    async fn local_close(&mut self) -> Result<(), Fail> {
        // 1. Start close protocol by setting state and sending FIN.
        self.cb.state = State::FinWait1;
        Sender::push_fin_and_wait_for_ack(&mut self.cb).await?;

        // 2. Got ACK to our FIN. Check if we also received a FIN from remote in the meantime.
        let state: State = self.cb.state;
        match state {
            State::FinWait1 => {
                self.cb.state = State::FinWait2;
                // Haven't received a FIN yet from remote, so wait.
                self.cb.receiver.wait_for_fin().await?;
            },
            State::Closing => self.cb.state = State::TimeWait,
            state => unreachable!("Cannot be in any other state at this point: {:?}", state),
        };
        // 3. TIMED_WAIT
        debug_assert_eq!(self.cb.state, State::TimeWait);
        trace!("socket options: {:?}", self.cb.socket_options.get_linger());
        let timeout: Duration = self.cb.socket_options.get_linger().unwrap_or(MSL * 2);
        yield_with_timeout(timeout).await;
        self.cb.state = State::Closed;
        Ok(())
    }

    async fn remote_already_closed(&mut self) -> Result<(), Fail> {
        // 0. Move state forward
        self.cb.state = State::LastAck;
        // 1. Send FIN and wait for ack before closing.
        Sender::push_fin_and_wait_for_ack(&mut self.cb).await?;
        self.cb.state = State::Closed;
        Ok(())
    }

    pub async fn push(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        let mut runtime: SharedDemiRuntime = self.runtime.clone();
        let mut layer3_endpoint: SharedLayer3Endpoint = self.layer3_endpoint.clone();
        Sender::push(&mut self.cb, &mut layer3_endpoint, &mut runtime, buf).await
    }

    pub async fn pop(&mut self, size: Option<usize>) -> Result<DemiBuffer, Fail> {
        self.cb.receiver.pop(size).await
    }

    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.cb.local, self.cb.remote)
    }

    async fn background(self) {
        let mut me: Self = self.clone();
        let acknowledger = async_timer!("tcp::established::background::acknowledger", async {
            let mut layer3_endpoint: SharedLayer3Endpoint = me.layer3_endpoint.clone();
            Receiver::acknowledger(&mut me.cb, &mut layer3_endpoint).await
        })
        .fuse();
        pin_mut!(acknowledger);

        let mut me2: Self = self.clone();
        let retransmitter = async_timer!("tcp::established::background::retransmitter", async {
            let mut layer3_endpoint: SharedLayer3Endpoint = me2.layer3_endpoint.clone();
            let mut runtime: SharedDemiRuntime = me2.runtime.clone();
            Sender::background_retransmitter(&mut me2.cb, &mut layer3_endpoint, &mut runtime).await
        })
        .fuse();
        pin_mut!(retransmitter);

        let mut me3: Self = self.clone();
        let sender = async_timer!("tcp::established::background::sender", async {
            let mut layer3_endpoint: SharedLayer3Endpoint = me3.layer3_endpoint.clone();
            let mut runtime: SharedDemiRuntime = me3.runtime.clone();
            Sender::background_sender(&mut me3.cb, &mut layer3_endpoint, &mut runtime).await
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
