// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::Icmpv4Header;
use crate::{
    inetstack::protocols::{
        ethernet2::Ethernet2Header,
        ipv4::Ipv4Header,
    },
    runtime::{
        memory::DemiBuffer,
        network::PacketBuf,
    },
};

/// Message for ICMP
pub struct Icmpv4Message {
    ethernet2_hdr: Ethernet2Header,
    ipv4_hdr: Ipv4Header,
    icmpv4_hdr: Icmpv4Header,
}

/// Associated Functions for Icmpv4Message
impl Icmpv4Message {
    /// Creates an ICMP message.
    pub fn new(ethernet2_hdr: Ethernet2Header, ipv4_hdr: Ipv4Header, icmpv4_hdr: Icmpv4Header) -> Self {
        Self {
            ethernet2_hdr,
            ipv4_hdr,
            icmpv4_hdr,
        }
    }
}

/// PacketBuf Trait Implementation for Icmpv4Message
impl PacketBuf for Icmpv4Message {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size() + self.ipv4_hdr.compute_size() + self.icmpv4_hdr.size()
    }

    fn body_size(&self) -> usize {
        0
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size = self.ethernet2_hdr.compute_size();
        let ipv4_hdr_size = self.ipv4_hdr.compute_size();
        let icmpv4_hdr_size = self.icmpv4_hdr.size();
        let mut cur_pos = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        let ipv4_payload_len = icmpv4_hdr_size;
        self.ipv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + ipv4_hdr_size)], ipv4_payload_len);
        cur_pos += ipv4_hdr_size;

        self.icmpv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + icmpv4_hdr_size)]);
    }

    fn take_body(&self) -> Option<DemiBuffer> {
        None
    }
}
