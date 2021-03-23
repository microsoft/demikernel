// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use std::marker::PhantomData;
use crate::{
    runtime::RuntimeBuf,
    fail::Fail,
    protocols::{
        ethernet2::frame::{
            Ethernet2Header,
        },
        ipv4::datagram::Ipv4Header,
    },
    runtime::PacketBuf,
};
use byteorder::{
    ByteOrder,
    NetworkEndian,
};
use std::{
    convert::TryInto,
};

#[allow(unused)]
const MAX_ICMPV4_DATAGRAM_SIZE: usize = 576;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Icmpv4Type2 {
    EchoReply { id: u16, seq_num: u16 },
    DestinationUnreachable,
    SourceQuench,
    RedirectMessage,
    EchoRequest { id: u16, seq_num: u16 },
    RouterAdvertisement,
    RouterSolicitation,
    TimeExceeded,
    BadIpHeader,
    Timestamp,
    TimestampReply,
}

impl Icmpv4Type2 {
    fn parse(type_byte: u8, rest_of_header: &[u8; 4]) -> Result<Self, Fail> {
        use Icmpv4Type2::*;
        match type_byte {
            0 => {
                let id = NetworkEndian::read_u16(&rest_of_header[0..2]);
                let seq_num = NetworkEndian::read_u16(&rest_of_header[2..4]);
                Ok(EchoReply { id, seq_num })
            },
            3 => Ok(DestinationUnreachable),
            4 => Ok(SourceQuench),
            5 => Ok(RedirectMessage),
            8 => {
                let id = NetworkEndian::read_u16(&rest_of_header[0..2]);
                let seq_num = NetworkEndian::read_u16(&rest_of_header[2..4]);
                Ok(EchoRequest { id, seq_num })
            },
            9 => Ok(RouterAdvertisement),
            10 => Ok(RouterSolicitation),
            11 => Ok(TimeExceeded),
            12 => Ok(BadIpHeader),
            13 => Ok(Timestamp),
            14 => Ok(TimestampReply),
            _ => Err(Fail::Malformed {
                details: "Invalid type byte",
            }),
        }
    }

    fn serialize(&self) -> (u8, [u8; 4]) {
        use Icmpv4Type2::*;
        match self {
            EchoReply { .. } => (0, [0u8; 4]),
            DestinationUnreachable => (3, [0u8; 4]),
            SourceQuench => (4, [0u8; 4]),
            RedirectMessage => (5, [0u8; 4]),
            EchoRequest { .. } => (8, [0u8; 4]),
            RouterAdvertisement => (9, [0u8; 4]),
            RouterSolicitation => (10, [0u8; 4]),
            TimeExceeded => (11, [0u8; 4]),
            BadIpHeader => (12, [0u8; 4]),
            Timestamp => (13, [0u8; 4]),
            TimestampReply => (14, [0u8; 4]),
        }
    }
}

pub struct Icmpv4Message<T> {
    pub ethernet2_hdr: Ethernet2Header,
    pub ipv4_hdr: Ipv4Header,
    pub icmpv4_hdr: Icmpv4Header,
    // TODO: Add a body enum when we need it.

    pub _body_marker: PhantomData<T>,
}

impl<T> PacketBuf<T> for Icmpv4Message<T> {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size()
            + self.ipv4_hdr.compute_size()
            + self.icmpv4_hdr.compute_size()
    }

    fn body_size(&self) -> usize {
        0
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size = self.ethernet2_hdr.compute_size();
        let ipv4_hdr_size = self.ipv4_hdr.compute_size();
        let icmpv4_hdr_size = self.icmpv4_hdr.compute_size();
        let mut cur_pos = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        let ipv4_payload_len = icmpv4_hdr_size;
        self.ipv4_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + ipv4_hdr_size)],
            ipv4_payload_len,
        );
        cur_pos += ipv4_hdr_size;

        self.icmpv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + icmpv4_hdr_size)]);
    }

    fn take_body(self) -> Option<T> {
        None
    }
}

pub const ICMPV4_HEADER_SIZE: usize = 8;

#[derive(Copy, Clone, Debug)]
pub struct Icmpv4Header {
    pub icmpv4_type: Icmpv4Type2,
    // TODO: Turn this into an enum on Icmpv4Type2 and then collapse the struct.
    pub code: u8,
}

impl Icmpv4Header {
    fn compute_size(&self) -> usize {
        ICMPV4_HEADER_SIZE
    }

    pub fn parse<T: RuntimeBuf>(mut buf: T) -> Result<(Self, T), Fail> {
        if buf.len() < ICMPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "ICMPv4 datagram too small for header",
            });
        }
        let hdr_buf: &[u8; ICMPV4_HEADER_SIZE] = &buf[..ICMPV4_HEADER_SIZE].try_into().unwrap();

        let type_byte = hdr_buf[0];
        let code = hdr_buf[1];
        let checksum = NetworkEndian::read_u16(&hdr_buf[2..4]);
        if checksum != icmpv4_checksum(hdr_buf, &buf[ICMPV4_HEADER_SIZE..]) {
            return Err(Fail::Malformed {
                details: "ICMPv4 checksum mismatch",
            });
        }
        let rest_of_header: &[u8; 4] = hdr_buf[4..8].try_into().unwrap();
        let icmpv4_type = Icmpv4Type2::parse(type_byte, rest_of_header)?;

        buf.adjust(ICMPV4_HEADER_SIZE);
        Ok((Self { icmpv4_type, code }, buf))
    }

    pub fn serialize(&self, buf: &mut [u8]) {
        let buf: &mut [u8; ICMPV4_HEADER_SIZE] =
            (&mut buf[..ICMPV4_HEADER_SIZE]).try_into().unwrap();
        let (type_byte, rest_of_header) = self.icmpv4_type.serialize();
        buf[0] = type_byte;
        buf[1] = self.code;
        // Skip the checksum for now.
        buf[4..8].copy_from_slice(&rest_of_header[..]);
        let checksum = icmpv4_checksum(buf, &[]);
        NetworkEndian::write_u16(&mut buf[2..4], checksum);
    }
}

fn icmpv4_checksum(buf: &[u8; ICMPV4_HEADER_SIZE], body: &[u8]) -> u16 {
    let mut state = 0xffffu32;
    state += NetworkEndian::read_u16(&buf[0..2]) as u32;
    // Skip the checksum.
    state += 0;
    state += NetworkEndian::read_u16(&buf[4..6]) as u32;
    state += NetworkEndian::read_u16(&buf[6..8]) as u32;

    let mut chunks_iter = body.chunks_exact(2);
    while let Some(chunk) = chunks_iter.next() {
        state += NetworkEndian::read_u16(chunk) as u32;
    }
    if let Some(&b) = chunks_iter.remainder().get(0) {
        state += NetworkEndian::read_u16(&[b, 0]) as u32;
    }

    while state > 0xFFFF {
        state -= 0xFFFF;
    }
    !state as u16
}
