// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        icmpv4::datagram::Icmpv4Header,
        ipv4::Ipv4Header,
        layer2::packet::PacketBuf,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Message for ICMP
pub struct Icmpv4Message {
    pkt: Option<DemiBuffer>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Icmpv4Message {
    /// Creates an ICMP message.
    pub fn new(ipv4_hdr: Ipv4Header, icmpv4_hdr: Icmpv4Header, mut data: DemiBuffer) -> Result<Self, Fail> {
        let ipv4_hdr_size: usize = ipv4_hdr.compute_size();
        let icmpv4_hdr_size: usize = icmpv4_hdr.size();
        let ipv4_payload_len: usize = icmpv4_hdr_size + data.len();

        // Add headers in reverse.
        data.prepend(icmpv4_hdr_size)?;
        let (hdr_buf, data_buf): (&mut [u8], &mut [u8]) = data.split_at_mut(icmpv4_hdr_size);
        icmpv4_hdr.serialize(hdr_buf, data_buf);

        data.prepend(ipv4_hdr_size)?;
        ipv4_hdr.serialize(&mut data, ipv4_payload_len);

        Ok(Self { pkt: Some(data) })
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// PacketBuf Trait Implementation for Icmpv4Message
impl PacketBuf for Icmpv4Message {
    fn take_body(&mut self) -> Option<DemiBuffer> {
        self.pkt.take()
    }
}
