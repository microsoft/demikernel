// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::{
        protocols::{
            ip::IpProtocol,
            ipv4::Ipv4Header,
        },
        test_helpers::{
            ALICE_IPV4,
            BOB_IPV4,
        },
    },
    runtime::memory::DemiBuffer,
};

//==============================================================================
// Helper Functions
//==============================================================================

/// Builds an IPv4 header.
/// NOTE: that we can use this function to create invalid IPv4 headers
fn build_ipv4_header(
    buf: &mut [u8],
    version: u8,
    ihl: u8,
    dscp: u8,
    ecn: u8,
    total_length: u16,
    id: u16,
    flags: u8,
    fragment_offset: u16,
    ttl: u8,
    protocol: u8,
    src_addr: &[u8],
    dest_addr: &[u8],
    mut checksum: Option<u16>,
) {
    // Version + IHL.
    buf[0] = ((version & 0xf) << 4) | (ihl & 0xf);

    // DSCP + ECN.
    buf[1] = ((dscp & 0x3f) << 2) | (ecn & 0x3);

    // Total Length.
    buf[2..4].copy_from_slice(&total_length.to_be_bytes());

    // ID.
    buf[4..6].copy_from_slice(&id.to_be_bytes());

    // Flags + Offset.
    let field: u16 = ((flags as u16 & 7) << 13) | (fragment_offset & 0x1fff);
    buf[6..8].copy_from_slice(&field.to_be_bytes());

    // Time to live.
    buf[8] = ttl;

    // Protocol.
    buf[9] = protocol;

    // Source address.
    buf[12..16].copy_from_slice(src_addr);

    // Destination address.
    buf[16..20].copy_from_slice(dest_addr);

    // Header checksum.
    if checksum.is_none() {
        checksum = Some(Ipv4Header::compute_checksum(&buf[..20]));
    }
    buf[10..12].copy_from_slice(&checksum.unwrap().to_be_bytes());
}

//==============================================================================
// Unit-Tests for Happy Path
//==============================================================================

/// Parses a well-formed IPv4 header.
#[test]
fn test_ipv4_header_parse_good() {
    const HEADER_MAX_SIZE: usize = (5 + 10) << 2;
    const PAYLOAD_SIZE: usize = 8;
    const DATAGRAM_SIZE: usize = HEADER_MAX_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];
    let data: [u8; PAYLOAD_SIZE] = [1, 2, 3, 4, 5, 6, 7, 8];
    let data_bytes: DemiBuffer = DemiBuffer::from_slice(&data).expect("'data' should fit in a DemiBuffer");

    for ihl in 5..15 {
        let header_size: usize = (ihl as usize) << 2;
        let datagram_size: usize = header_size + PAYLOAD_SIZE;
        build_ipv4_header(
            &mut buf[..header_size],
            4,
            ihl,
            0,
            0,
            datagram_size as u16,
            0,
            0x2,
            0,
            1,
            IpProtocol::UDP as u8,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Payload
        buf[header_size..datagram_size].copy_from_slice(&data);

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf[..datagram_size]).expect("'buf' should fit");
        match Ipv4Header::parse(buf_bytes) {
            Ok((ipv4_hdr, datagram)) => {
                assert_eq!(ipv4_hdr.get_src_addr(), ALICE_IPV4);
                assert_eq!(ipv4_hdr.get_dest_addr(), BOB_IPV4);
                assert_eq!(ipv4_hdr.get_protocol(), IpProtocol::UDP);
                assert_eq!(datagram.len(), PAYLOAD_SIZE);
                assert_eq!(datagram[..], data_bytes[..]);
            },
            Err(e) => assert!(false, "{:?}", e),
        }
    }
}

//==============================================================================
// Unit-Tests for Invalid Path
//==============================================================================

/// Parses a malformed IPv4 header with invalid version number.
#[test]
fn test_ipv4_header_parse_invalid_version() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over all invalid version numbers.
    for version in [0, 1, 2, 3, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
        build_ipv4_header(
            &mut buf,
            version,
            5,
            0,
            0,
            DATAGRAM_SIZE as u16,
            0,
            0x2,
            0,
            1,
            IpProtocol::UDP as u8,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
        match Ipv4Header::parse(buf_bytes) {
            Ok(_) => assert!(false, "parsed ipv4_header with invalid version={:?}", version),
            Err(_) => {},
        };
    }
}

/// Parses a malformed IPv4 header with invalid internet header length.
#[test]
fn test_ipv4_header_parse_invalid_ihl() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over invalid values for IHL.
    for ihl in [0, 1, 2, 3, 4] {
        build_ipv4_header(
            &mut buf,
            4,
            ihl,
            0,
            0,
            DATAGRAM_SIZE as u16,
            0,
            0x2,
            0,
            1,
            IpProtocol::UDP as u8,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
        match Ipv4Header::parse(buf_bytes) {
            Ok(_) => assert!(false, "parsed ipv4 header with invalid ihl={:?}", ihl),
            Err(_) => {},
        };
    }
}

/// Parses a malformed IPv4 header with invalid total length field.
#[test]
fn test_ipv4_header_parse_invalid_total_length() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over invalid values for IHL.
    for total_length in 0..20 {
        build_ipv4_header(
            &mut buf,
            4,
            5,
            0,
            0,
            total_length,
            0,
            0x2,
            0,
            1,
            IpProtocol::UDP as u8,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
        match Ipv4Header::parse(buf_bytes) {
            Ok(_) => assert!(false, "parsed ipv4 header with invalid total_length={:?}", total_length),
            Err(_) => {},
        };
    }
}

/// Parses a malformed IPv4 header with invalid flags field.
#[test]
fn test_ipv4_header_parse_invalid_flags() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Flags field must have bit 3 zeroed as it is marked Reserved.
    let flags: u8 = 0x4;
    build_ipv4_header(
        &mut buf,
        4,
        5,
        0,
        0,
        DATAGRAM_SIZE as u16,
        0x1d,
        flags,
        0,
        1,
        IpProtocol::UDP as u8,
        &ALICE_IPV4.octets(),
        &BOB_IPV4.octets(),
        None,
    );

    // Do it.
    let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
    match Ipv4Header::parse(buf_bytes) {
        Ok(_) => assert!(false, "parsed ipv4 header with invalid flags={:?}", flags),
        Err(_) => {},
    };
}

/// Parses a malformed IPv4 header with invalid time to live field.
#[test]
fn test_ipv4_header_parse_invalid_ttl() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Datagrams with zeroed TTL values must me dropped.
    let ttl: u8 = 0x0;
    build_ipv4_header(
        &mut buf,
        4,
        5,
        0,
        0,
        DATAGRAM_SIZE as u16,
        0,
        0x2,
        0,
        ttl,
        IpProtocol::UDP as u8,
        &ALICE_IPV4.octets(),
        &BOB_IPV4.octets(),
        None,
    );

    // Do it.
    let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
    match Ipv4Header::parse(buf_bytes) {
        Ok(_) => assert!(false, "parsed ipv4 header with invalid ttl={:?}", ttl),
        Err(_) => {},
    };
}

/// Parses a malformed IPv4 header with invalid protocol field.
#[test]
fn test_ipv4_header_parse_invalid_protocol() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over invalid values for protocol.
    for protocol in 144..252 {
        build_ipv4_header(
            &mut buf,
            4,
            5,
            0,
            0,
            DATAGRAM_SIZE as u16,
            0,
            0x2,
            0,
            1,
            protocol,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
        match Ipv4Header::parse(buf_bytes) {
            Ok(_) => assert!(false, "parsed ipv4 header with invalid protocol={:?}", protocol),
            Err(_) => {},
        };
    }
}

/// Parses a malformed IPv4 header with invalid checksum.
#[test]
fn test_ipv4_header_parse_invalid_header_checksum() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Datagrams with invalid header checksum must me dropped.
    let hdr_checksum: u16 = 0x1;
    build_ipv4_header(
        &mut buf,
        4,
        5,
        0,
        0,
        DATAGRAM_SIZE as u16,
        0,
        0x2,
        0,
        1,
        IpProtocol::UDP as u8,
        &ALICE_IPV4.octets(),
        &BOB_IPV4.octets(),
        Some(hdr_checksum),
    );

    // Do it.
    let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
    match Ipv4Header::parse(buf_bytes) {
        Ok(_) => assert!(
            false,
            "parsed ipv4 header with invalid header checksum={:?}",
            hdr_checksum
        ),
        Err(_) => {},
    };
}

//==============================================================================
// Unit-Tests for Unsupported Paths
//==============================================================================

/// Parses a malformed IPv4 header with unsupported DSCP field.
#[test]
fn test_ipv4_header_parse_unsupported_dscp() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over unsupported values for DSCP.
    for dscp in 1..63 {
        build_ipv4_header(
            &mut buf,
            4,
            5,
            dscp,
            0,
            DATAGRAM_SIZE as u16,
            0,
            0x2,
            0,
            1,
            IpProtocol::UDP as u8,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
        match Ipv4Header::parse(buf_bytes) {
            Ok(_) => {},
            Err(_) => panic!("dscp field should be ignored (dscp={:?})", dscp),
        };
    }
}

/// Parses a malformed IPv4 header with unsupported ECN field.
#[test]
fn test_ipv4_header_parse_unsupported_ecn() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over unsupported values for ECN.
    for ecn in 1..3 {
        build_ipv4_header(
            &mut buf,
            4,
            5,
            0,
            ecn,
            DATAGRAM_SIZE as u16,
            0,
            0x2,
            0,
            1,
            IpProtocol::UDP as u8,
            &ALICE_IPV4.octets(),
            &BOB_IPV4.octets(),
            None,
        );

        // Do it.
        let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
        match Ipv4Header::parse(buf_bytes) {
            Ok(_) => {},
            Err(_) => panic!("ecn field should be ignored (ecn={:?})", ecn),
        };
    }
}

/// Parses a malformed IPv4 header with unsupported fragmentation fields.
///
/// TODO: Drop this test once we support fragmentation.
#[test]
fn test_ipv4_header_parse_unsupported_fragmentation() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Fragmented packets are unsupported.
    // Fragments are detected by having either the MF bit set in Flags or a non-zero Fragment Offset field.
    let flags: u8 = 0x1; // Set MF bit.
    build_ipv4_header(
        &mut buf,
        4,
        5,
        0,
        0,
        DATAGRAM_SIZE as u16,
        0x1d,
        flags,
        0,
        1,
        IpProtocol::UDP as u8,
        &ALICE_IPV4.octets(),
        &BOB_IPV4.octets(),
        None,
    );

    // Do it.
    let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
    match Ipv4Header::parse(buf_bytes) {
        Ok(_) => assert!(
            false,
            "parsed ipv4 header with Flags={:?}. Do we support it now?",
            flags,
        ),
        Err(_) => {},
    };

    // Fragmented packets are unsupported.
    // Fragments are detected by having either the MF bit set in Flags or a non-zero Fragment Offset field.
    let fragment_offset: u16 = 1;
    build_ipv4_header(
        &mut buf,
        4,
        5,
        0,
        0,
        DATAGRAM_SIZE as u16,
        0x1d,
        0x2,
        fragment_offset,
        1,
        IpProtocol::UDP as u8,
        &ALICE_IPV4.octets(),
        &BOB_IPV4.octets(),
        None,
    );

    // Do it.
    let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
    match Ipv4Header::parse(buf_bytes) {
        Ok(_) => assert!(
            false,
            "parsed ipv4 header with fragment_offset={:?}. Do we support it now?",
            fragment_offset,
        ),
        Err(_) => {},
    };
}

/// Parses a malformed IPv4 header with unsupported protocol field.
///
/// TODO: Drop this test once we support them.
#[test]
fn test_ipv4_header_parse_unsupported_protocol() {
    const HEADER_SIZE: usize = 20;
    const PAYLOAD_SIZE: usize = 0;
    const DATAGRAM_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
    let mut buf: [u8; DATAGRAM_SIZE] = [0; DATAGRAM_SIZE];

    // Iterate over unsupported values for fragment flags.
    for protocol in 0..143 {
        match protocol {
            // Skip supported protocols.
            1 | 6 | 17 => continue,
            _ => {
                build_ipv4_header(
                    &mut buf,
                    4,
                    5,
                    0,
                    0,
                    DATAGRAM_SIZE as u16,
                    0,
                    0x2,
                    0,
                    1,
                    protocol,
                    &ALICE_IPV4.octets(),
                    &BOB_IPV4.octets(),
                    None,
                );

                // Do it.
                let buf_bytes: DemiBuffer = DemiBuffer::from_slice(&buf).expect("'buf' should fit in a DemiBuffer");
                match Ipv4Header::parse(buf_bytes) {
                    Ok(_) => assert!(
                        false,
                        "parsed ipv4 header with protocol={:?}. Do we support it now?",
                        protocol,
                    ),
                    Err(_) => {},
                };
            },
        };
    }
}
