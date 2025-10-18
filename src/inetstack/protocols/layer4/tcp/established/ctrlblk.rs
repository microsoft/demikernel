// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::{
        config::TcpConfig,
        protocols::layer4::tcp::established::{
            congestion_control_state::CongestionControlState, delivery_state::DeliveryState,
            flow_control_state::FlowControlState,
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
    // Connection management state, which mainly includes
    // connection constants for TCP
    pub connection_management: ConnectionManagementState,
    pub delivery: DeliveryState,

    // Flow control state
    pub flow_control: FlowControlState,

    // Congestion control state
    pub congestion_control: CongestionControlState,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl ControlBlock {
    pub fn new(
        connection_management: ConnectionManagementState,
        delivery: DeliveryState,
        flow_control: FlowControlState,
        congestion_control: CongestionControlState,
    ) -> Self {
        Self {
            connection_management,
            delivery,
            flow_control,
            congestion_control,
        }
    }
}
