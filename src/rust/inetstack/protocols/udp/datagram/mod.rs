// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    inetstack::protocols::{
        ipv4::Ipv4Header,
        layer2::packet::PacketBuf,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
    },
};

//======================================================================================================================
// Exports
//======================================================================================================================

pub use header::{
    UdpHeader,
    UDP_HEADER_SIZE,
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// UDP Datagram
#[derive(Debug)]
pub struct UdpDatagram {
    pkt: Option<DemiBuffer>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

// Associate Functions for UDP Datagrams
impl UdpDatagram {
    /// Creates a UDP packet.
    pub fn new(
        ipv4_hdr: Ipv4Header,
        udp_hdr: UdpHeader,
        mut pkt: DemiBuffer,
        checksum_offload: bool,
    ) -> Result<Self, Fail> {
        let ipv4_hdr_size: usize = ipv4_hdr.compute_size();
        let udp_hdr_size: usize = udp_hdr.size();

        // Attach headers in reverse.
        pkt.prepend(udp_hdr_size)?;
        let (hdr_buf, data_buf): (&mut [u8], &mut [u8]) = pkt[..].split_at_mut(udp_hdr_size);
        udp_hdr.serialize(hdr_buf, &ipv4_hdr, data_buf, checksum_offload);
        let ipv4_payload_len: usize = pkt.len();
        pkt.prepend(ipv4_hdr_size)?;
        ipv4_hdr.serialize(&mut pkt[..ipv4_hdr_size], ipv4_payload_len);

        Ok(Self { pkt: Some(pkt) })
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// Packet Buffer Trait Implementation for UDP Datagrams
impl PacketBuf for UdpDatagram {
    /// Returns the payload of the target UDP datagram.
    fn take_body(&mut self) -> Option<DemiBuffer> {
        self.pkt.take()
    }
}

//======================================================================================================================
// Unit Tests
//======================================================================================================================

#[cfg(test)]
mod test {
    use self::header::UDP_HEADER_SIZE;
    use crate::inetstack::protocols::{
        ip::IpProtocol,
        ipv4::IPV4_HEADER_MIN_SIZE,
        udp::datagram::*,
    };
    use ::anyhow::Result;
    use ::std::net::Ipv4Addr;

    #[test]
    fn test_udp_datagram_header_serialization() -> Result<()> {
        // Total header size.
        const HEADER_SIZE: usize = (IPV4_HEADER_MIN_SIZE as usize) + UDP_HEADER_SIZE;

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
        let data: DemiBuffer =
            DemiBuffer::from_slice_with_headroom(&bytes, HEADER_SIZE).expect("bytes should be shorter than u16::MAX");

        // Build expected header.
        let mut hdr: [u8; HEADER_SIZE] = [0; HEADER_SIZE];
        ipv4_hdr.serialize(&mut hdr[0..IPV4_HEADER_MIN_SIZE as usize], UDP_HEADER_SIZE + data.len());
        udp_hdr.serialize(
            &mut hdr[IPV4_HEADER_MIN_SIZE as usize..],
            &ipv4_hdr,
            &data,
            checksum_offload,
        );

        // Output buffer.
        let mut datagram: UdpDatagram = UdpDatagram::new(ipv4_hdr, udp_hdr, data, checksum_offload)?;
        let buf: DemiBuffer = match datagram.take_body() {
            Some(body) => body,
            _ => {
                let cause = format!("No body in PacketBuf to transmit");
                anyhow::bail!(cause);
            },
        };
        // Do it.
        crate::ensure_eq!(buf[..HEADER_SIZE], hdr[..]);

        Ok(())
    }
}
