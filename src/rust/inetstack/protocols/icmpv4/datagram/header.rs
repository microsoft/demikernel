// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::protocol::Icmpv4Type2;
use crate::runtime::{
    fail::Fail,
    memory::DemiBuffer,
};
use ::libc::EBADMSG;
use ::std::convert::TryInto;

/// Size of ICMPv4 Headers (in bytes)
pub const ICMPV4_HEADER_SIZE: usize = 8;

#[derive(Copy, Clone, Debug)]
pub struct Icmpv4Header {
    protocol: Icmpv4Type2,
    // TODO: Turn this into an enum on Icmpv4Type2 and then collapse the struct.
    code: u8,
}

/// Associate Functions for Icmpv4Header
impl Icmpv4Header {
    /// Creates a header for a ICMP Message.
    pub fn new(icmpv4_type: Icmpv4Type2, code: u8) -> Self {
        Self {
            protocol: icmpv4_type,
            code,
        }
    }

    /// Returns the size of the target ICMPv4 header.
    pub fn size(&self) -> usize {
        ICMPV4_HEADER_SIZE
    }

    pub fn parse(mut buf: DemiBuffer) -> Result<(Self, DemiBuffer), Fail> {
        if buf.len() < ICMPV4_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "ICMPv4 datagram too small for header"));
        }
        let hdr_buf: &[u8; ICMPV4_HEADER_SIZE] = &buf[..ICMPV4_HEADER_SIZE].try_into().unwrap();

        let type_byte: u8 = hdr_buf[0];
        let code: u8 = hdr_buf[1];
        let checksum: u16 = u16::from_be_bytes([hdr_buf[2], hdr_buf[3]]);
        if checksum != Self::checksum(hdr_buf, &buf[ICMPV4_HEADER_SIZE..]) {
            return Err(Fail::new(EBADMSG, "ICMPv4 checksum mismatch"));
        }
        let rest_of_header: &[u8; 4] = hdr_buf[4..8].try_into().unwrap();
        let icmpv4_type: Icmpv4Type2 = Icmpv4Type2::parse(type_byte, rest_of_header)?;

        buf.adjust(ICMPV4_HEADER_SIZE)?;
        Ok((
            Self {
                protocol: icmpv4_type,
                code,
            },
            buf,
        ))
    }

    pub fn serialize(&self, buf: &mut [u8], data: &[u8]) {
        let buf: &mut [u8; ICMPV4_HEADER_SIZE] = (&mut buf[..ICMPV4_HEADER_SIZE]).try_into().unwrap();
        let (type_byte, rest_of_header) = self.protocol.serialize();
        buf[0] = type_byte;
        buf[1] = self.code;
        // Skip the checksum for now.
        buf[4..8].copy_from_slice(&rest_of_header[..]);
        let checksum: u16 = Self::checksum(buf, data);
        buf[2..4].copy_from_slice(&checksum.to_be_bytes());
    }

    fn checksum(buf: &[u8; ICMPV4_HEADER_SIZE], body: &[u8]) -> u16 {
        let mut state: u32 = 0xffff;
        state += u16::from_be_bytes([buf[0], buf[1]]) as u32;
        // Skip the checksum.
        state += 0;
        state += u16::from_be_bytes([buf[4], buf[5]]) as u32;
        state += u16::from_be_bytes([buf[6], buf[7]]) as u32;

        let mut chunks_iter = body.chunks_exact(2);
        while let Some(chunk) = chunks_iter.next() {
            state += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
        }
        if let Some(&b) = chunks_iter.remainder().get(0) {
            state += u16::from_be_bytes([b, 0]) as u32;
        }

        while state > 0xFFFF {
            state -= 0xFFFF;
        }
        !state as u16
    }

    pub fn get_protocol(&self) -> Icmpv4Type2 {
        self.protocol
    }
}
