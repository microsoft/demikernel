// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::layer3::ip::IpProtocol,
    runtime::{fail::Fail, memory::DemiBuffer},
};
use ::libc::EBADMSG;
use ::std::{net::Ipv4Addr, slice::ChunksExact};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Size of a UDP header (in bytes).
pub const UDP_HEADER_SIZE: usize = 8;

//======================================================================================================================
// Structures
//======================================================================================================================

/// UDP Datagram Header
#[derive(Debug)]
pub struct UdpHeader {
    /// Port used on sender side (optional).
    src_port: u16,
    /// Port used receiver side.
    dest_port: u16,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

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

    /// Parses and strips the UDP header off of the packet in [buf].
    pub fn parse_and_strip(
        src_ipv4_addr: &Ipv4Addr,
        dst_ipv4_addr: &Ipv4Addr,
        buf: &mut DemiBuffer,
        checksum_offload: bool,
    ) -> Result<Self, Fail> {
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
                if checksum != Self::checksum(src_ipv4_addr, dst_ipv4_addr, hdr_buf, payload_buf) {
                    return Err(Fail::new(EBADMSG, "UDP checksum mismatch"));
                }
            }
        }

        let header: UdpHeader = Self::new(src_port, dest_port);
        buf.adjust(UDP_HEADER_SIZE)
            .expect("Buffer should be at least long enough to hold the UDP header");
        Ok(header)
    }

    /// Serializes and prepends the UDP header on to the packet in [buf].
    pub fn serialize_and_attach(
        &self,
        buf: &mut DemiBuffer,
        src_ipv4_addr: &Ipv4Addr,
        dst_ipv4_addr: &Ipv4Addr,
        checksum_offload: bool,
    ) {
        // Create room for the header in the packet.
        buf.prepend(UDP_HEADER_SIZE).expect("Should have enough headroom");
        let buf_size_bytes: usize = buf.len();

        // Split the packet up into two slices: one for the header and one for the payload.
        let (hdr_buf, payload): (&mut [u8], &mut [u8]) = buf[..].split_at_mut(UDP_HEADER_SIZE);

        let fixed_buf: &mut [u8; UDP_HEADER_SIZE] = (&mut hdr_buf[..UDP_HEADER_SIZE]).try_into().unwrap();

        // Write source port.
        fixed_buf[0..2].copy_from_slice(&self.src_port.to_be_bytes());

        // Write destination port.
        fixed_buf[2..4].copy_from_slice(&self.dest_port.to_be_bytes());

        // Write payload length.
        fixed_buf[4..6].copy_from_slice(&(buf_size_bytes as u16).to_be_bytes());

        // Write checksum.
        let checksum: u16 = if checksum_offload {
            0
        } else {
            Self::checksum(src_ipv4_addr, dst_ipv4_addr, &fixed_buf[..], payload)
        };
        fixed_buf[6..8].copy_from_slice(&checksum.to_be_bytes());
        trace!("UDP header: {:?} packet size: {:?} bytes", self, buf_size_bytes);
    }

    /// Computes the checksum of a UDP datagram.
    ///
    /// This is the 16-bit one's complement of the one's complement sum of a
    /// pseudo header of information from the IP header, the UDP header, and the
    /// data,  padded  with zero octets at the end (if  necessary)  to  make  a
    /// multiple of two octets.
    ///
    /// TODO: Write a unit test for this function.
    fn checksum(src_ipv4_addr: &Ipv4Addr, dst_ipv4_addr: &Ipv4Addr, udp_hdr: &[u8], data: &[u8]) -> u16 {
        let mut state: u32 = 0xffff;

        // Source address (4 bytes)
        let src_octets: [u8; 4] = src_ipv4_addr.octets();
        state += u16::from_be_bytes([src_octets[0], src_octets[1]]) as u32;
        state += u16::from_be_bytes([src_octets[2], src_octets[3]]) as u32;

        // Destination address (4 bytes)
        let dst_octets: [u8; 4] = dst_ipv4_addr.octets();
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

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use crate::inetstack::protocols::layer4::udp::header::*;
    use ::anyhow::Result;
    use ::std::net::Ipv4Addr;

    #[test]
    fn test_udp_header_serialization() -> Result<()> {
        const UDP_HEADER_SIZE: usize = 8;
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);

        // Build fake UDP header.
        let src_port: u16 = 0x32;
        let dest_port: u16 = 0x45;
        let checksum_offload: bool = true;
        let udp_hdr: UdpHeader = UdpHeader::new(src_port, dest_port);

        let payload_data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];
        let mut buf: DemiBuffer = DemiBuffer::from_slice_with_headroom(&payload_data, UDP_HEADER_SIZE)?;

        udp_hdr.serialize_and_attach(&mut buf, &src_addr, &dst_addr, checksum_offload);
        crate::ensure_eq!(buf[..UDP_HEADER_SIZE], [0x0, 0x32, 0x0, 0x45, 0x0, 0x10, 0x0, 0x0]);

        Ok(())
    }

    #[test]
    fn test_udp_header_parsing() -> Result<()> {
        // Build fake IPv4 header.
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);

        // Build fake UDP header.
        let src_port: u16 = 0x32;
        let checksum_offload: bool = true;
        let dest_port: u16 = 0x45;
        let hdr: [u8; 8] = [0x0, 0x32, 0x0, 0x45, 0x0, 0x10, 0x0, 0x0];

        let payload_data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];
        let mut buf: DemiBuffer = DemiBuffer::from_slice(&[hdr, payload_data].concat())?;

        match UdpHeader::parse_and_strip(&src_addr, &dst_addr, &mut buf, checksum_offload) {
            Ok(udp_hdr) => {
                crate::ensure_eq!(udp_hdr.src_port(), src_port);
                crate::ensure_eq!(udp_hdr.dest_port(), dest_port);
                crate::ensure_eq!(buf.len(), 8);
            },
            Err(e) => anyhow::bail!("could not parse: {:?}", e),
        };

        Ok(())
    }
}
