// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::layer3::PacketBuf,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};
use ::std::net::Ipv4Addr;

//======================================================================================================================
// Exports
//======================================================================================================================

pub use header::{
    UdpHeader,
    UDP_HEADER_SIZE,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// UDP Datagram
#[derive(Debug)]
pub struct UdpDatagram {
    pkt: Option<DemiBuffer>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

// Associate Functions for UDP Datagrams
impl UdpDatagram {
    /// Creates a UDP packet.
    pub fn new(
        src_ipv4_addr: &Ipv4Addr,
        dst_ipv4_addr: &Ipv4Addr,
        udp_hdr: UdpHeader,
        mut pkt: DemiBuffer,
        checksum_offload: bool,
    ) -> Result<Self, Fail> {
        let udp_hdr_bytes: usize = udp_hdr.size();

        // Attach headers in reverse.
        pkt.prepend(udp_hdr_bytes)?;
        let (hdr_buf, data_buf): (&mut [u8], &mut [u8]) = pkt[..].split_at_mut(udp_hdr_bytes);

        udp_hdr.serialize(hdr_buf, src_ipv4_addr, dst_ipv4_addr, data_buf, checksum_offload);

        Ok(Self { pkt: Some(pkt) })
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Packet Buffer Trait Implementation for UDP Datagrams
impl PacketBuf for UdpDatagram {
    /// Returns the payload of the target UDP datagram.
    fn take_body(&mut self) -> Option<DemiBuffer> {
        self.pkt.take()
    }
}
