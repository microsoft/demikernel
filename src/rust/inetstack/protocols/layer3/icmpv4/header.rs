// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{compute_generic_checksum, fold16, layer3::icmpv4::protocol::Icmpv4Type2},
    runtime::{fail::Fail, memory::DemiBuffer},
};
use ::libc::EBADMSG;

//======================================================================================================================
// Constants
//======================================================================================================================

/// Size of ICMPv4 Headers (in bytes)
pub const ICMPV4_HEADER_SIZE: usize = 8;

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(Copy, Clone, Debug)]
pub struct Icmpv4Header {
    protocol: Icmpv4Type2,
    // TODO: Turn this into an enum on Icmpv4Type2 and then collapse the struct.
    code: u8,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate Functions for Icmpv4Header
impl Icmpv4Header {
    /// Creates a header for a ICMP Message.
    pub fn new(icmpv4_type: Icmpv4Type2, code: u8) -> Self {
        Self {
            protocol: icmpv4_type,
            code,
        }
    }

    /// Strips and parses the ICMP header from the packet in [buf].
    pub fn parse_and_strip(buf: &mut DemiBuffer) -> Result<Self, Fail> {
        if buf.len() < ICMPV4_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "ICMPv4 datagram too small for header"));
        }
        let hdr_buf: &[u8; ICMPV4_HEADER_SIZE] = &buf[..ICMPV4_HEADER_SIZE].try_into().unwrap();

        let type_byte: u8 = hdr_buf[0];
        let code: u8 = hdr_buf[1];
        if Self::compute_checksum(hdr_buf, &buf[ICMPV4_HEADER_SIZE..]) != 0 {
            return Err(Fail::new(EBADMSG, "ICMPv4 checksum mismatch"));
        }
        let rest_of_header: &[u8; 4] = hdr_buf[4..8].try_into().unwrap();
        let icmpv4_type: Icmpv4Type2 = Icmpv4Type2::parse(type_byte, rest_of_header)?;

        buf.adjust(ICMPV4_HEADER_SIZE)?;
        Ok(Self {
            protocol: icmpv4_type,
            code,
        })
    }

    /// Serializes and prepends the ICMP header into the packet in [buf]. This function assumes that the packet has
    /// sufficient headroom to fit the ICMP header.
    pub fn serialize_and_attach(&self, buf: &mut DemiBuffer) {
        buf.prepend(ICMPV4_HEADER_SIZE).expect("Should have headroom");

        let (type_byte, rest_of_header) = self.protocol.serialize();
        buf[0] = type_byte;
        buf[1] = self.code;
        // Skip the checksum for now.
        buf[2] = 0;
        buf[3] = 0;
        buf[4..8].copy_from_slice(&rest_of_header[..]);
        let (hdr_buf, payload): (&[u8], &[u8]) = buf[..].split_at(ICMPV4_HEADER_SIZE as usize);
        let checksum: u16 = Self::compute_checksum(hdr_buf, payload);
        buf[2..4].copy_from_slice(&checksum.to_be_bytes());
    }

    /// Computes the checksum of the target ICMPv4 header. We can assume that the header buffer is the right size
    /// because we just split it from the body up above.
    fn compute_checksum(buf: &[u8], body: &[u8]) -> u16 {
        let mut state: u32 = compute_generic_checksum(buf, None);
        state = compute_generic_checksum(body, Some(state));

        fold16(state)
    }

    pub fn get_protocol(&self) -> Icmpv4Type2 {
        self.protocol
    }
}
