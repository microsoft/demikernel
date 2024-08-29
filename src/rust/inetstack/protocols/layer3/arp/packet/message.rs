// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        layer2::packet::PacketBuf,
        layer3::arp::packet::ArpHeader,
        MAX_HEADER_SIZE,
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
    pub fn new(pdu: ArpHeader) -> Result<Self, Fail> {
        let arp_pdu_size: usize = pdu.compute_size();
        // Leave this here until we get rid of this data structure.
        let mut pkt: DemiBuffer = DemiBuffer::new_with_headroom(0, MAX_HEADER_SIZE as u16);
        pkt.prepend(arp_pdu_size)?;
        pdu.serialize(&mut pkt[..arp_pdu_size]);

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
