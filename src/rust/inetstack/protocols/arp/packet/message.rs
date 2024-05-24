// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        arp::packet::ArpHeader,
        ethernet2::Ethernet2Header,
    },
    runtime::{
        memory::DemiBuffer,
        network::PacketBuf,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Debug)]
pub struct ArpMessage {
    ethernet2_hdr: Ethernet2Header,
    header: ArpHeader,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ArpMessage {
    /// Creates an ARP message.
    pub fn new(header: Ethernet2Header, pdu: ArpHeader) -> Self {
        Self {
            ethernet2_hdr: header,
            header: pdu,
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl PacketBuf for ArpMessage {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size() + self.header.compute_size()
    }

    fn body_size(&self) -> usize {
        0
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size = self.ethernet2_hdr.compute_size();
        let arp_pdu_size = self.header.compute_size();
        let mut cur_pos = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        self.header.serialize(&mut buf[cur_pos..(cur_pos + arp_pdu_size)]);
    }

    fn take_body(&mut self) -> Option<DemiBuffer> {
        None
    }
}
