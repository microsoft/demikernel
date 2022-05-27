// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::IoUringRuntime;
use crate::Ipv4Addr;
use ::arrayvec::ArrayVec;
use ::runtime::{
    memory::Buffer,
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        consts::RECEIVE_BATCH_SIZE,
        types::MacAddress,
        NetworkRuntime,
        PacketBuf,
    },
};

//==============================================================================
// Trait Implementations
//==============================================================================

/// Network Runtime Trait Implementation for I/O User Ring Runtime
impl NetworkRuntime for IoUringRuntime {
    // TODO: Rely on a default implementation for this.
    fn transmit(&self, _pkt: impl PacketBuf) {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn receive(&self) -> ArrayVec<Box<dyn Buffer>, RECEIVE_BATCH_SIZE> {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn local_link_addr(&self) -> MacAddress {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn local_ipv4_addr(&self) -> Ipv4Addr {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn arp_options(&self) -> ArpConfig {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn tcp_options(&self) -> TcpConfig {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn udp_options(&self) -> UdpConfig {
        unreachable!()
    }
}
