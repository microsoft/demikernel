// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        arp::packet::ArpHeader,
        layer1::PacketBuf,
        layer2::Ethernet2Header,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
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
    pub fn new(header: Ethernet2Header, pdu: ArpHeader) -> Result<Self, Fail> {
        let eth_hdr_size: usize = header.compute_size();
        let arp_pdu_size: usize = pdu.compute_size();
        let mut pkt: DemiBuffer = DemiBuffer::new_with_headroom(0, (eth_hdr_size + arp_pdu_size) as u16);
        pkt.prepend(arp_pdu_size)?;
        pdu.serialize(&mut pkt[..arp_pdu_size]);

        pkt.prepend(eth_hdr_size)?;
        header.serialize(&mut pkt[..eth_hdr_size]);
        Ok(Self { pkt: Some(pkt) })
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
