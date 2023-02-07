// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::{
        ip::IpProtocol,
        ipv4::Ipv4Header,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};
use ::libc::EBADMSG;
use ::std::convert::TryInto;
use std::slice::ChunksExact;

//==============================================================================
// Constants
//==============================================================================

/// Size of a UDP header (in bytes).
pub const UDP_HEADER_SIZE: usize = 8;

//==============================================================================
// Structures
//==============================================================================

/// UDP Datagram Header
#[derive(Debug)]
pub struct UdpHeader {
    /// Port used on sender side (optional).
    src_port: u16,
    /// Port used receiver side.
    dest_port: u16,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate functions for UDP Datagram Headers
impl UdpHeader {
    /// Creates a UDP header.
    pub fn new(src_port: u16, dest_port: u16) -> Self {
        Self { src_port, dest_port }
    }

    /// Returns the source port stored in the target UDP header.
    pub fn src_port(&self) -> u16 {
        self.src_port
    }

    /// Returns the destination port stored in the target UDP header.
    pub fn dest_port(&self) -> u16 {
        self.dest_port
    }

    /// Returns the size of the target UDP header (in bytes).
    pub fn size(&self) -> usize {
        UDP_HEADER_SIZE
    }

    /// Parses a byte slice into a UDP header.
    pub fn parse_from_slice<'a>(
        ipv4_hdr: &Ipv4Header,
        buf: &'a [u8],
        checksum_offload: bool,
    ) -> Result<(Self, &'a [u8]), Fail> {
        // Malformed header.
        if buf.len() < UDP_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "UDP segment too small"));
        }

        // Deserialize buffer.
        let hdr_buf: &[u8] = &buf[..UDP_HEADER_SIZE];
        let src_port: u16 = u16::from_be_bytes([hdr_buf[0], hdr_buf[1]]);
        let dest_port: u16 = u16::from_be_bytes([hdr_buf[2], hdr_buf[3]]);
        let length: usize = u16::from_be_bytes([hdr_buf[4], hdr_buf[5]]) as usize;
        if length != buf.len() {
            return Err(Fail::new(EBADMSG, "UDP length mismatch"));
        }

        // Checksum payload.
        if !checksum_offload {
            let payload_buf: &[u8] = &buf[UDP_HEADER_SIZE..];
            let checksum: u16 = u16::from_be_bytes([hdr_buf[6], hdr_buf[7]]);
            // Check if we should skip checksum verification.
            if checksum != 0 {
                // No, so check if checksum value matches what we expect.
                if checksum != Self::checksum(&ipv4_hdr, hdr_buf, payload_buf) {
                    return Err(Fail::new(EBADMSG, "UDP checksum mismatch"));
                }
            }
        }

        let header: UdpHeader = Self::new(src_port, dest_port);
        Ok((header, &buf[UDP_HEADER_SIZE..]))
    }

    /// Parses a buffer into a UDP header.
    pub fn parse(ipv4_hdr: &Ipv4Header, buf: DemiBuffer, checksum_offload: bool) -> Result<(Self, DemiBuffer), Fail> {
        match Self::parse_from_slice(ipv4_hdr, &buf[..], checksum_offload) {
            Ok((udp_hdr, bytes)) => Ok((udp_hdr, DemiBuffer::from_slice(bytes)?)),
            Err(e) => Err(e),
        }
    }

    /// Serializes the target UDP header.
    pub fn serialize(&self, buf: &mut [u8], ipv4_hdr: &Ipv4Header, data: &[u8], checksum_offload: bool) {
        let fixed_buf: &mut [u8; UDP_HEADER_SIZE] = (&mut buf[..UDP_HEADER_SIZE]).try_into().unwrap();

        // Write source port.
        fixed_buf[0..2].copy_from_slice(&self.src_port.to_be_bytes());

        // Write destination port.
        fixed_buf[2..4].copy_from_slice(&self.dest_port.to_be_bytes());

        // Write payload length.
        fixed_buf[4..6].copy_from_slice(&((UDP_HEADER_SIZE + data.len()) as u16).to_be_bytes());

        // Write checksum.
        let checksum: u16 = if checksum_offload {
            0
        } else {
            Self::checksum(ipv4_hdr, &fixed_buf[..], data)
        };
        fixed_buf[6..8].copy_from_slice(&checksum.to_be_bytes());
    }

    /// Computes the checksum of a UDP datagram.
    ///
    /// This is the 16-bit one's complement of the one's complement sum of a
    /// pseudo header of information from the IP header, the UDP header, and the
    /// data,  padded  with zero octets at the end (if  necessary)  to  make  a
    /// multiple of two octets.
    ///
    /// TODO: Write a unit test for this function.
    fn checksum(ipv4_hdr: &Ipv4Header, udp_hdr: &[u8], data: &[u8]) -> u16 {
        let mut state: u32 = 0xffff;

        // Source address (4 bytes)
        let src_octets: [u8; 4] = ipv4_hdr.get_src_addr().octets();
        state += u16::from_be_bytes([src_octets[0], src_octets[1]]) as u32;
        state += u16::from_be_bytes([src_octets[2], src_octets[3]]) as u32;

        // Destination address (4 bytes)
        let dst_octets: [u8; 4] = ipv4_hdr.get_dest_addr().octets();
        state += u16::from_be_bytes([dst_octets[0], dst_octets[1]]) as u32;
        state += u16::from_be_bytes([dst_octets[2], dst_octets[3]]) as u32;

        // Padding zeros (1 byte) and UDP protocol number (1 byte)
        state += u16::from_be_bytes([0, IpProtocol::UDP as u8]) as u32;

        // UDP segment length (2 bytes)
        state += (udp_hdr.len() + data.len()) as u32;

        // Switch to UDP header.
        let fixed_header: &[u8; UDP_HEADER_SIZE] = udp_hdr.try_into().unwrap();

        // Source port (2 bytes)
        state += u16::from_be_bytes([fixed_header[0], fixed_header[1]]) as u32;

        // Destination port (2 bytes)
        state += u16::from_be_bytes([fixed_header[2], fixed_header[3]]) as u32;

        // Payload Length (2 bytes)
        state += u16::from_be_bytes([fixed_header[4], fixed_header[5]]) as u32;

        // Checksum (2 bytes, all zeros)
        state += 0;

        // Payload.
        let mut chunks_iter: ChunksExact<u8> = data.chunks_exact(2);
        while let Some(chunk) = chunks_iter.next() {
            state += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
        }
        // Pad with zeros with payload has an odd number of bytes.
        if let Some(&b) = chunks_iter.remainder().get(0) {
            state += u16::from_be_bytes([b, 0]) as u32;
        }

        // NOTE: We don't need to subtract out 0xFFFF as we accumulate the sum.
        // Since we use a u32 for intermediate state, we would need 2^16
        // additions to overflow. This is well beyond the reach of the largest
        // jumbo frames. The upshot is that the compiler can then optimize this
        // final loop into a single branch-free code.
        while state > 0xFFFF {
            state -= 0xFFFF;
        }
        !state as u16
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod test {
    use super::*;
    use ::std::net::Ipv4Addr;

    /// Builds a fake Ipv4 Header.
    fn ipv4_header() -> Ipv4Header {
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);
        let protocol: IpProtocol = IpProtocol::UDP;
        Ipv4Header::new(src_addr, dst_addr, protocol)
    }

    /// Tets UDP serialization.
    #[test]
    fn test_udp_header_serialization() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake UDP header.
        let src_port: u16 = 0x32;
        let dest_port: u16 = 0x45;
        let checksum_offload: bool = true;
        let udp_hdr: UdpHeader = UdpHeader::new(src_port, dest_port);

        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        // Output buffer.
        let mut buf: [u8; 8] = [0; 8];

        // Do it.
        udp_hdr.serialize(&mut buf, &ipv4_hdr, &data, checksum_offload);
        assert_eq!(buf, [0x0, 0x32, 0x0, 0x45, 0x0, 0x10, 0x0, 0x0]);
    }

    /// Tests UDP parsing.
    #[test]
    fn test_udp_header_parsing() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake UDP header.
        let src_port: u16 = 0x32;
        let checksum_offload: bool = true;
        let dest_port: u16 = 0x45;
        let hdr: [u8; 8] = [0x0, 0x32, 0x0, 0x45, 0x0, 0x10, 0x0, 0x0];

        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        // Input buffer.
        let buf: Vec<u8> = [hdr, data].concat();

        // Do it.
        match UdpHeader::parse_from_slice(&ipv4_hdr, &buf, checksum_offload) {
            Ok((udp_hdr, buf)) => {
                assert_eq!(udp_hdr.src_port(), src_port);
                assert_eq!(udp_hdr.dest_port(), dest_port);
                assert_eq!(buf.len(), 8);
            },
            Err(e) => {
                assert!(false, "{:?}", e);
            },
        }
    }
}
