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
        memory::DemiBuffer,
        network::PacketBuf,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Message for ICMP
pub struct Icmpv4Message {
    ethernet2_hdr: Ethernet2Header,
    ipv4_hdr: Ipv4Header,
    icmpv4_hdr: Icmpv4Header,
    data: DemiBuffer,
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
        data: DemiBuffer,
    ) -> Self {
        Self {
            ethernet2_hdr,
            ipv4_hdr,
            icmpv4_hdr,
            data,
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// PacketBuf Trait Implementation for Icmpv4Message
impl PacketBuf for Icmpv4Message {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size() + self.ipv4_hdr.compute_size() + self.icmpv4_hdr.size()
    }

    fn body_size(&self) -> usize {
        self.data.len()
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size: usize = self.ethernet2_hdr.compute_size();
        let ipv4_hdr_size: usize = self.ipv4_hdr.compute_size();
        let icmpv4_hdr_size: usize = self.icmpv4_hdr.size();
        let mut cur_pos: usize = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        let ipv4_payload_len: usize = icmpv4_hdr_size + self.body_size();
        self.ipv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + ipv4_hdr_size)], ipv4_payload_len);
        cur_pos += ipv4_hdr_size;

        self.icmpv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + icmpv4_hdr_size)], &self.data);
    }

    fn take_body(&mut self) -> Option<DemiBuffer> {
        Some(self.data.clone())
    }
}
