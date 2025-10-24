// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::inetstack::protocols::layer4::tcp::established::{
    congestion_control_state::CongestionControlState, connection_management_state::ConnectionManagementState,
    flow_control_state::FlowControlState, ordered_delivery_state::OrderedDeliveryState,
};

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
pub struct ControlBlock {
    // Connection management state, which mainly includes
    // connection constants for TCP
    pub connection_management: ConnectionManagementState,
    pub delivery: OrderedDeliveryState,

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
        delivery: OrderedDeliveryState,
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
