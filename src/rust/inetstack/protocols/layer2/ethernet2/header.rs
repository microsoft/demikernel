// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::layer2::EtherType2,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::types::MacAddress,
    },
};
use ::libc::EBADMSG;

//======================================================================================================================
// Constants
//======================================================================================================================

pub const ETHERNET2_HEADER_SIZE: usize = 14;
pub const MIN_PAYLOAD_SIZE: usize = 46;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Clone, Debug)]
pub struct Ethernet2Header {
    // Bytes 0..6
    dst_addr: MacAddress,
    // Bytes 6..12
    src_addr: MacAddress,
    // Bytes 12..14
    ether_type: EtherType2,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl Ethernet2Header {
    /// Creates a header for an Ethernet frame.
    pub fn new(dst_addr: MacAddress, src_addr: MacAddress, ether_type: EtherType2) -> Self {
        Self {
            dst_addr,
            src_addr,
            ether_type,
        }
    }

    /// Parse and strip the ethernet header from the packet in [buf].
    pub fn parse_and_strip(buf: &mut DemiBuffer) -> Result<Self, Fail> {
        if buf.len() < ETHERNET2_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "frame too small"));
        }
        let hdr_buf = &buf[..ETHERNET2_HEADER_SIZE];
        let dst_addr = MacAddress::from_bytes(&hdr_buf[0..6]);
        let src_addr = MacAddress::from_bytes(&hdr_buf[6..12]);
        let ether_type = EtherType2::try_from(u16::from_be_bytes([hdr_buf[12], hdr_buf[13]]))?;

        buf.adjust(ETHERNET2_HEADER_SIZE)?;
        Ok(Self {
            dst_addr,
            src_addr,
            ether_type,
        })
    }

    /// Create and prepend the ethernet header onto the packet in [buf].
    pub fn serialize_and_attach(&self, buf: &mut DemiBuffer) {
        buf.prepend(ETHERNET2_HEADER_SIZE).expect("Should have enough headroom");
        buf[0..6].copy_from_slice(&self.dst_addr.octets());
        buf[6..12].copy_from_slice(&self.src_addr.octets());
        buf[12..14].copy_from_slice(&(self.ether_type as u16).to_be_bytes());
    }

    pub fn dst_addr(&self) -> MacAddress {
        self.dst_addr
    }

    pub fn src_addr(&self) -> MacAddress {
        self.src_addr
    }

    pub fn ether_type(&self) -> EtherType2 {
        self.ether_type
    }
}
