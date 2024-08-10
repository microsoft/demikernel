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
    pkt: Option<DemiBuffer>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ArpMessage {
    /// Creates an ARP message.
    pub fn new(header: Ethernet2Header, pdu: ArpHeader) -> Self {
        let eth_hdr_size: usize = header.compute_size();
        let arp_pdu_size: usize = pdu.compute_size();
        let mut pkt: DemiBuffer = DemiBuffer::new((eth_hdr_size + arp_pdu_size) as u16);
        let mut cur_pos = 0;

        header.serialize(&mut pkt[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        pdu.serialize(&mut pkt[cur_pos..(cur_pos + arp_pdu_size)]);
        Self { pkt: Some(pkt) }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl PacketBuf for ArpMessage {
    fn take_body(&mut self) -> Option<DemiBuffer> {
        self.pkt.take()
    }
}
