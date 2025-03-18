// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        config::TcpConfig,
        protocols::{
            layer3::NetworkLayer,
            layer4::tcp::established::{congestion_control, Receiver, Sender},
        },
    },
    runtime::network::socket::option::TcpSocketOptions,
};
use ::std::net::SocketAddrV4;

//======================================================================================================================
// Structures
//======================================================================================================================

/// TCP Connection State.
/// Note: This ControlBlock structure is only used after we've reached the ESTABLISHED state, so states LISTEN,
/// SYN_RCVD, and SYN_SENT aren't included here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
    CloseWait,
    LastAck,
    Closed,
}

/// Transmission control block for representing our TCP connection.
/// This struct has only public members because includes state for both the send and receive path and is accessed by
/// both.
pub struct ControlBlock<T: NetworkLayer> {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub tcp_config: TcpConfig,
    pub socket_options: TcpSocketOptions,
    pub state: State,
    pub sender: Sender,
    pub receiver: Receiver,
    pub flow_state: T::FlowState,

    // Congestion control trait implementation we're currently using.
    // TODO: Consider switching this to a static implementation to avoid V-table call overhead.
    pub congestion_control_algorithm: Box<dyn congestion_control::CongestionControl>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl<T: NetworkLayer> ControlBlock<T> {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        tcp_config: TcpConfig,
        socket_options: TcpSocketOptions,
        sender: Sender,
        receiver: Receiver,
        flow_state: T::FlowState,
        congestion_control_algorithm: Box<dyn congestion_control::CongestionControl>,
    ) -> Self {
        Self {
            local,
            remote,
            tcp_config,
            socket_options,
            state: State::Established,
            sender,
            receiver,
            flow_state,
            congestion_control_algorithm,
        }
    }
}
