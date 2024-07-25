// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::ipv4::Ipv4Header,
    runtime::{
        fail::Fail,
        memory::{
            DemiBuffer,
        },
    },
};
use ::byteorder::{
    ByteOrder,
    NetworkEndian,
};
use ::libc::EBADMSG;
use ::std::convert::TryInto;
use std::net::{SocketAddrV4, Ipv4Addr};

//==============================================================================
// Constants
//==============================================================================

/// Size of a TCPMig header (in bytes).
pub const TCPMIG_HEADER_SIZE: usize = 28;
pub const RPS_SIGNAL_HEADER_SIZE: usize = 20;

pub const MAGIC_NUMBER: u32 = 0xCAFEDEAD;
pub const RPS_SIGNAL_SIGNATURE: u32 = 0xABCDABCD;

const FLAG_LOAD_BIT: u8 = 0;
const FLAG_NEXT_FRAGMENT: u8 = 1;
const STAGE_BIT_SHIFT: u8 = 4;

//==============================================================================
// Structures
//==============================================================================

//
//  Header format:
//  (The first 8 bytes of this header act as a UDP header for DPDK compatibility)
//  
//  Offset  Size    Data
//  0       2       Source Demikernel Process Port (UDP Source Port field)
//  2       2       Dest Demikernel Process Port (UDP Dest Port field)
//  4       2       Length (UDP Length field)
//  6       2       Checksum (UDP Checksum field)
//  8       4       Magic Number
//  12      4       Origin Server IP
//  16      2       Origin Server Port
//  18      4       Client IP
//  22      2       Client Port
//  24      2       Fragment Offset
//  26      1       Flags + Stage
//  27      1       Zero (unused)
//
//  TOTAL 28
//
//
//  Flags format:
//  Bit number      Flag
//  0               LOAD - Instructs the switch to load the entry into the migration tables.
//  1               NEXT_FRAGMENT - Whether there is a fragment after this.
//  4-7             Migration Stage.
//  

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpMigHeader {
    /// Client-facing address of the origin server.
    pub origin: SocketAddrV4,
    /// Client's address.
    pub client: SocketAddrV4,
    pub length: u16,
    pub fragment_offset: u16,

    pub flag_load: bool,
    pub flag_next_fragment: bool,

    pub stage: MigrationStage,

    pub source_udp_port: u16,
    pub dest_udp_port: u16,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStage {
    None = 0, // TcpMigHeader's flag cannot be 0, so p4 program checks this to filter out non-TCPMig packets
    Rejected,
    PrepareMigration,
    PrepareMigrationAck,
    ConnectionState,
    ConnectionStateAck,

    // Heartbeat Protocol.
    // HeartbeatUpdate = 12,
    // HeartbeatResponse = 13,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl TcpMigHeader {
    /// Creates a TcpMigration header.
    pub fn new(
        origin: SocketAddrV4, client: SocketAddrV4, payload_length: u16, stage: MigrationStage,
        source_udp_port: u16, dest_udp_port: u16
    ) -> Self {
        Self {
            origin,
            client,
            length: TCPMIG_HEADER_SIZE as u16 + payload_length,
            fragment_offset: 0,
            flag_load: false,
            flag_next_fragment: false,
            stage,
            source_udp_port,
            dest_udp_port,
        }
    }

    pub fn get_source_udp_port(&self) -> u16 {
        self.source_udp_port
    }

    pub fn swap_src_dst_port(&mut self) {
        let temp: u16 = self.source_udp_port;
        self.source_udp_port = self.dest_udp_port;
        self.dest_udp_port = temp;
    }

    pub fn is_tcpmig(buf: &[u8]) -> bool {
        buf.len() >= TCPMIG_HEADER_SIZE &&
            NetworkEndian::read_u32(&buf[8..12]) == MAGIC_NUMBER
    }

    pub fn is_rps_signal(buf: &[u8]) -> bool {
        buf.len() >= RPS_SIGNAL_HEADER_SIZE &&
            NetworkEndian::read_u32(&buf[8..12]) == RPS_SIGNAL_SIGNATURE
    }

    /// Returns the size of the TcpMigration header (in bytes).
    pub fn size(&self) -> usize {
        TCPMIG_HEADER_SIZE
    }

    /// Parses a byte slice into a TcpMigration header.
    pub fn parse_from_slice<'a>(
        ipv4_hdr: &Ipv4Header,
        buf: &'a [u8],
    ) -> Result<(Self, &'a [u8]), Fail> {
        // Malformed header.
        if buf.len() < TCPMIG_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "TCPMig segment too small"));
        }
        if NetworkEndian::read_u32(&buf[8..12]) != MAGIC_NUMBER {
            return Err(Fail::new(EBADMSG, "not a TCPMig segment"));
        }

        // Deserialize buffer.
        let hdr_buf: &[u8] = &buf[..TCPMIG_HEADER_SIZE];

        let source_udp_port = NetworkEndian::read_u16(&hdr_buf[0..2]);
        let dest_udp_port = NetworkEndian::read_u16(&hdr_buf[2..4]);
        let length = NetworkEndian::read_u16(&hdr_buf[4..6]);
        let checksum = NetworkEndian::read_u16(&hdr_buf[6..8]);

        let origin = SocketAddrV4::new(
            Ipv4Addr::new(hdr_buf[12], hdr_buf[13], hdr_buf[14], hdr_buf[15]),
            NetworkEndian::read_u16(&hdr_buf[16..18]),
        );
        let client = SocketAddrV4::new(
            Ipv4Addr::new(hdr_buf[18], hdr_buf[19], hdr_buf[20], hdr_buf[21]),
            NetworkEndian::read_u16(&hdr_buf[22..24]),
        );

        let fragment_offset = NetworkEndian::read_u16(&hdr_buf[24..26]);

        // Flags.
        let flags = hdr_buf[26];
        let flag_load = (flags & (1 << FLAG_LOAD_BIT)) != 0;
        let flag_next_fragment = (flags & (1 << FLAG_NEXT_FRAGMENT)) != 0;

        let stage = (flags & 0xF0) >> STAGE_BIT_SHIFT;
        println!("Received stage value: {}", stage);
        let stage: MigrationStage = match stage.try_into() {
            Ok(stage) => stage,
            Err(e) => return Err(Fail::new(EBADMSG, &format!("Invalid TCPMig stage: {}", e))),
        };

        // Checksum payload.
        let payload_buf: &[u8] = &buf[TCPMIG_HEADER_SIZE..];
        if checksum != 0 && checksum != Self::checksum(&ipv4_hdr, hdr_buf, payload_buf) {
            return Err(Fail::new(EBADMSG, "TCPMig checksum mismatch"));
        }

        let header = Self {
            origin,
            client,
            length,
            fragment_offset,
            flag_load,
            flag_next_fragment,
            stage,
            source_udp_port,
            dest_udp_port,
        };

        Ok((header, &buf[TCPMIG_HEADER_SIZE..]))
    }

    /// Parses a buffer into a TcpMigration header.
    pub fn parse(ipv4_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(Self, DemiBuffer), Fail> {
        match Self::parse_from_slice(ipv4_hdr, &buf) {
            Ok((hdr, bytes)) => Ok((hdr, DemiBuffer::from_slice(bytes).expect("slice should fit in DemiBuffer"))),
            
            Err(e) => Err(e),
        }
    }

    /// Serializes the target TcpMigration header.
    pub fn serialize(&self, buf: &mut [u8], ipv4_hdr: &Ipv4Header, data: &[u8]) {
        let fixed_buf: &mut [u8; TCPMIG_HEADER_SIZE] = (&mut buf[..TCPMIG_HEADER_SIZE]).try_into().unwrap();

        NetworkEndian::write_u16(&mut fixed_buf[0..2], self.source_udp_port);
        NetworkEndian::write_u16(&mut fixed_buf[2..4], self.dest_udp_port);
        NetworkEndian::write_u16(&mut fixed_buf[4..6], self.length);
        NetworkEndian::write_u16(&mut fixed_buf[6..8], 0);

        NetworkEndian::write_u32(&mut fixed_buf[8..12], MAGIC_NUMBER);

        fixed_buf[12..16].copy_from_slice(&self.origin.ip().octets());
        NetworkEndian::write_u16(&mut fixed_buf[16..18], self.origin.port());
        fixed_buf[18..22].copy_from_slice(&self.client.ip().octets());
        NetworkEndian::write_u16(&mut fixed_buf[22..24], self.client.port());

        NetworkEndian::write_u16(&mut fixed_buf[24..26], self.fragment_offset);
        fixed_buf[26] = self.serialize_flags_and_stage();
        fixed_buf[27] = 0;

        let checksum = Self::checksum(ipv4_hdr, fixed_buf, data);
        NetworkEndian::write_u16(&mut fixed_buf[6..8], checksum);
    }

    #[inline(always)]
    fn serialize_flags_and_stage(&self) -> u8 {
        (if self.flag_load {1} else {0} << FLAG_LOAD_BIT)
        | (if self.flag_next_fragment {1} else {0} << FLAG_NEXT_FRAGMENT)
        | ((self.stage as u8) << STAGE_BIT_SHIFT)
    }

    /// Computes the checksum of a TcpMigration segment.
    fn checksum(ipv4_hdr: &Ipv4Header, migration_hdr: &[u8], data: &[u8]) -> u16 {
        let data_chunks_rem = data.chunks_exact(2).remainder();
        let data_chunks_rem: [u8; 2] = match data_chunks_rem.len() {
            0 => [0, 0],
            1 => [data_chunks_rem[0], 0],
            _ => unreachable!()
        };

        ipv4_hdr.get_src_addr().octets().chunks_exact(2)
        .chain(ipv4_hdr.get_dest_addr().octets().chunks_exact(2))
        .chain(migration_hdr[..6].chunks_exact(2))
        .chain(migration_hdr[8..].chunks_exact(2))
        .chain(data.chunks_exact(2))
        .chain(data_chunks_rem.chunks_exact(2))
        .fold(0, |sum: u16, e| sum.wrapping_add(NetworkEndian::read_u16(e)))
        .wrapping_neg()
    }
}

//======================================================================================================================
// Standard Library Trait Implementations
//======================================================================================================================

impl From<MigrationStage> for u8 {
    fn from(value: MigrationStage) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for MigrationStage {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, u8> {
        use MigrationStage::*;
        match value {
            0 => Ok(None),
            1 => Ok(Rejected),
            2 => Ok(PrepareMigration),
            3 => Ok(PrepareMigrationAck),
            4 => Ok(ConnectionState),
            5 => Ok(ConnectionStateAck),

            // 12 => Ok(HeartbeatUpdate),
            // 13 => Ok(HeartbeatResponse),

            e => Err(e),
        }
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

/*#[cfg(test)]
 mod test {
    use crate::inetstack::protocols::ip::IpProtocol;
    use super::*;
    use ::std::net::Ipv4Addr;
    use std::str::FromStr;

    /// Builds a fake Ipv4 Header.
    fn ipv4_header() -> Ipv4Header {
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);
        let protocol: IpProtocol = IpProtocol::UDP;
        Ipv4Header::new(src_addr, dst_addr, protocol)
    }

    fn tcpmig_header() -> TcpMigHeader {
        TcpMigHeader {
            origin: SocketAddrV4::from_str("198.0.0.1:20000").unwrap(),
            remote: SocketAddrV4::from_str("18.45.32.67:19465").unwrap(),
            payload_length: 8,
            fragment_offset: 2,
            flag_load: false,
            flag_next_fragment: true,
            stage: MigrationStage::PrepareMigration,
        }
    }

    const CHECKSUM: u16 = 48981;
    const HDR_BYTES: [u8; TCPMIG_HEADER_SIZE] = [
        0xCA, 0xFE, 0xDE, 0xAD,
        198, 0, 0, 1, 0x4e, 0x20, // origin
        18, 45, 32, 67, 0x4c, 0x09, // remote
        0, 8, // payload length
        0, 2, // fragment offset
        0b0010_0010, // stage + flags
        0,
        ((CHECKSUM & 0xFF00) >> 8) as u8, (CHECKSUM & 0xFF) as u8,
    ];

    /// Tests Checksum
    #[test]
    fn test_tcpmig_header_checksum() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake TCPMig header.
        let hdr: &[u8] = &HDR_BYTES;

        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        let checksum = TcpMigHeader::checksum(&ipv4_hdr, hdr, &data);
        assert_eq!(checksum, 48981);
    }

    /// Tests TCPMig serialization.
    #[test]
    fn test_tcpmig_header_serialization() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake TCPMig header.
        let hdr = tcpmig_header();
        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        // Output buffer.
        let mut buf: [u8; TCPMIG_HEADER_SIZE] = [0; TCPMIG_HEADER_SIZE];

        hdr.serialize(&mut buf, &ipv4_hdr, &data);
        assert_eq!(buf, HDR_BYTES);
    }

    /// Tests TCPMig parsing.
    #[test]
    fn test_tcpmig_header_parsing() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake TCPMig header.
        let origin = SocketAddrV4::from_str("198.0.0.1:20000").unwrap();
        let remote = SocketAddrV4::from_str("18.45.32.67:19465").unwrap();
        let hdr = HDR_BYTES;

        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        // Input buffer.
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&hdr);
        buf.extend_from_slice(&data);

        match TcpMigHeader::parse_from_slice(&ipv4_hdr, &buf) {
            Ok((hdr, buf)) => {
                assert_eq!(hdr.origin, origin);
                assert_eq!(hdr.remote, remote);
                assert_eq!(hdr.payload_length, 8);
                assert_eq!(hdr.fragment_offset, 2);
                assert_eq!(hdr.flag_load, false);
                assert_eq!(hdr.flag_next_fragment, true);
                assert_eq!(hdr.stage, MigrationStage::PrepareMigration);
                assert_eq!(buf.len(), 8);
            },
            Err(e) => {
                assert!(false, "{:?}", e);
            },
        }
    }
}
 */