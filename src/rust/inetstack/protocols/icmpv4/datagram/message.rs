// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        ethernet2::Ethernet2Header,
        icmpv4::datagram::Icmpv4Header,
        ipv4::Ipv4Header,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::PacketBuf,
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
    pub fn new(
        ethernet2_hdr: Ethernet2Header,
        ipv4_hdr: Ipv4Header,
        icmpv4_hdr: Icmpv4Header,
        mut data: DemiBuffer,
    ) -> Result<Self, Fail> {
        let eth_hdr_size: usize = ethernet2_hdr.compute_size();
        let ipv4_hdr_size: usize = ipv4_hdr.compute_size();
        let icmpv4_hdr_size: usize = icmpv4_hdr.size();
        let ipv4_payload_len: usize = icmpv4_hdr_size + data.len();

        // Add headers in reverse.
        data.prepend(icmpv4_hdr_size)?;
        let (hdr_buf, data_buf): (&mut [u8], &mut [u8]) = data.split_at_mut(icmpv4_hdr_size);
        icmpv4_hdr.serialize(hdr_buf, data_buf);

        data.prepend(ipv4_hdr_size)?;
        ipv4_hdr.serialize(&mut data, ipv4_payload_len);
        data.prepend(eth_hdr_size)?;
        ethernet2_hdr.serialize(&mut data);

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
