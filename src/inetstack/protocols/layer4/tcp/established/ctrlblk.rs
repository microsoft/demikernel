// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        config::TcpConfig,
        protocols::layer4::tcp::established::{
            congestion_control, congestion_control_state::CongestionControlState, Receiver, Sender,
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

/// State block representing connection management parameters in a TCP connection.
/// This struct has only public members since these parameters must be read by all TCP
/// modules.
pub struct ConnectionManagementState {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub tcp_config: TcpConfig,
    pub socket_options: TcpSocketOptions,
    pub state: State,
}

impl ConnectionManagementState {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        tcp_config: TcpConfig,
        socket_options: TcpSocketOptions,
    ) -> Self {
        Self {
            local,
            remote,
            tcp_config,
            socket_options,
            state: State::Established,
        }
    }
}

/// Transmission control block for representing our TCP connection.
/// This struct has only public members because includes state for both the send and receive path and is accessed by
/// both.
pub struct ControlBlock {
    pub connection_management: ConnectionManagementState,
    pub sender: Sender,
    pub receiver: Receiver,

    // Congestion control trait implementation we're currently using.
    // TODO: Consider switching this to a static implementation to avoid V-table call overhead.
    pub congestion_control: CongestionControlState,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl ControlBlock {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        tcp_config: TcpConfig,
        socket_options: TcpSocketOptions,
        sender: Sender,
        receiver: Receiver,
        congestion_control_algorithm: Box<dyn congestion_control::CongestionControl>,
    ) -> Self {
        Self {
            connection_management: ConnectionManagementState::new(local, remote, tcp_config, socket_options),
            sender,
            receiver,
            congestion_control: CongestionControlState::new(congestion_control_algorithm),
        }
    }
}
