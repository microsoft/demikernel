// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::protocols::ethernet2::EtherType2,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::types::MacAddress,
    },
};
use ::libc::EBADMSG;
use ::std::convert::{
    TryFrom,
    TryInto,
};

pub const ETHERNET2_HEADER_SIZE: usize = 14;
pub const MIN_PAYLOAD_SIZE: usize = 46;

#[derive(Clone, Debug)]
pub struct Ethernet2Header {
    // Bytes 0..6
    dst_addr: MacAddress,
    // Bytes 6..12
    src_addr: MacAddress,
    // Bytes 12..14
    ether_type: EtherType2,
}

impl Ethernet2Header {
    /// Creates a header for an Ethernet frame.
    pub fn new(dst_addr: MacAddress, src_addr: MacAddress, ether_type: EtherType2) -> Self {
        Self {
            dst_addr,
            src_addr,
            ether_type,
        }
    }

    pub fn compute_size(&self) -> usize {
        ETHERNET2_HEADER_SIZE
    }

    pub fn parse(mut buf: DemiBuffer) -> Result<(Self, DemiBuffer), Fail> {
        if buf.len() < ETHERNET2_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "frame too small"));
        }
        let hdr_buf = &buf[..ETHERNET2_HEADER_SIZE];
        let dst_addr = MacAddress::from_bytes(&hdr_buf[0..6]);
        let src_addr = MacAddress::from_bytes(&hdr_buf[6..12]);
        let ether_type = EtherType2::try_from(u16::from_be_bytes([hdr_buf[12], hdr_buf[13]]))?;
        let hdr = Self {
            dst_addr,
            src_addr,
            ether_type,
        };

        buf.adjust(ETHERNET2_HEADER_SIZE)?;
        Ok((hdr, buf))
    }

    pub fn serialize(&self, buf: &mut [u8]) {
        let buf: &mut [u8; ETHERNET2_HEADER_SIZE] = buf.try_into().unwrap();
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
