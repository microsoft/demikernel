// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        ethernet2::Ethernet2Header,
        ipv4::Ipv4Header,
    },
    runtime::{
        memory::DemiBuffer,
        network::PacketBuf,
    },
};

//======================================================================================================================
// Exports
//======================================================================================================================

pub use header::UdpHeader;

//======================================================================================================================
// Structures
//======================================================================================================================

/// UDP Datagram
#[derive(Debug)]
pub struct UdpDatagram {
    /// Ethernet header.
    ethernet2_hdr: Ethernet2Header,
    /// IPv4 header.
    ipv4_hdr: Ipv4Header,
    /// UDP header.
    udp_hdr: UdpHeader,
    /// Payload
    data: DemiBuffer,
    /// Offload checksum to hardware?
    checksum_offload: bool,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

// Associate Functions for UDP Datagrams
impl UdpDatagram {
    /// Creates a UDP packet.
    pub fn new(
        ethernet2_hdr: Ethernet2Header,
        ipv4_hdr: Ipv4Header,
        udp_hdr: UdpHeader,
        data: DemiBuffer,
        checksum_offload: bool,
    ) -> Self {
        Self {
            ethernet2_hdr,
            ipv4_hdr,
            udp_hdr,
            data,
            checksum_offload,
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Packet Buffer Trait Implementation for UDP Datagrams
impl PacketBuf for UdpDatagram {
    /// Computes the header size of the target UDP datagram.
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size() + self.ipv4_hdr.compute_size() + self.udp_hdr.size()
    }

    /// Computes the payload size of the target UDP datagram.
    fn body_size(&self) -> usize {
        self.data.len()
    }

    /// Serializes the header of the target UDP datagram.
    fn write_header(&self, buf: &mut [u8]) {
        let mut cur_pos: usize = 0;
        let eth_hdr_size: usize = self.ethernet2_hdr.compute_size();
        let udp_hdr_size: usize = self.udp_hdr.size();
        let ipv4_payload_len: usize = udp_hdr_size + self.data.len();

        // Ethernet header.
        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        // IPV4 header.
        let ipv4_hdr_size = self.ipv4_hdr.compute_size();
        self.ipv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + ipv4_hdr_size)], ipv4_payload_len);
        cur_pos += ipv4_hdr_size;

        // UDP header.
        self.udp_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + udp_hdr_size)],
            &self.ipv4_hdr,
            &self.data[..],
            self.checksum_offload,
        );
    }

    /// Returns the payload of the target UDP datagram.
    fn take_body(&mut self) -> Option<DemiBuffer> {
        Some(self.data.clone())
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use self::header::UDP_HEADER_SIZE;
    use crate::{
        inetstack::protocols::{
            ethernet2::{
                EtherType2,
                ETHERNET2_HEADER_SIZE,
            },
            ip::IpProtocol,
            ipv4::IPV4_HEADER_MIN_SIZE,
            udp::datagram::*,
        },
        runtime::network::types::MacAddress,
    };
    use ::anyhow::Result;
    use ::std::net::Ipv4Addr;

    #[test]
    fn test_udp_datagram_header_serialization() -> Result<()> {
        // Total header size.
        const HEADER_SIZE: usize = ETHERNET2_HEADER_SIZE + (IPV4_HEADER_MIN_SIZE as usize) + UDP_HEADER_SIZE;

        // Build fake Ethernet2 header.
        let dst_addr: MacAddress = MacAddress::new([0xd, 0xe, 0xa, 0xd, 0x0, 0x0]);
        let src_addr: MacAddress = MacAddress::new([0xb, 0xe, 0xe, 0xf, 0x0, 0x0]);
        let ether_type: EtherType2 = EtherType2::Ipv4;
        let ethernet2_hdr: Ethernet2Header = Ethernet2Header::new(dst_addr, src_addr, ether_type);

        // Build fake Ipv4 header.
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);
        let protocol: IpProtocol = IpProtocol::UDP;
        let ipv4_hdr: Ipv4Header = Ipv4Header::new(src_addr, dst_addr, protocol);

        // Build fake UDP header.
        let src_port: u16 = 0x32;
        let dest_port: u16 = 0x45;
        let checksum_offload: bool = true;
        let udp_hdr: UdpHeader = UdpHeader::new(src_port, dest_port);

        // Payload.
        let bytes: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];
        let data: DemiBuffer = DemiBuffer::from_slice(&bytes).expect("bytes should be shorter than u16::MAX");

        // Build expected header.
        let mut hdr: [u8; HEADER_SIZE] = [0; HEADER_SIZE];
        ethernet2_hdr.serialize(&mut hdr[0..ETHERNET2_HEADER_SIZE]);
        ipv4_hdr.serialize(
            &mut hdr[ETHERNET2_HEADER_SIZE..(ETHERNET2_HEADER_SIZE + (IPV4_HEADER_MIN_SIZE as usize))],
            UDP_HEADER_SIZE + data.len(),
        );
        udp_hdr.serialize(
            &mut hdr[(ETHERNET2_HEADER_SIZE + (IPV4_HEADER_MIN_SIZE as usize))..],
            &ipv4_hdr,
            &data,
            checksum_offload,
        );

        // Output buffer.
        let mut buf: [u8; HEADER_SIZE] = [0; HEADER_SIZE];

        let datagram: UdpDatagram = UdpDatagram::new(ethernet2_hdr, ipv4_hdr, udp_hdr, data, checksum_offload);

        // Do it.
        datagram.write_header(&mut buf);
        crate::ensure_eq!(buf, hdr);

        Ok(())
    }
}
