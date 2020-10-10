// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

#[cfg(test)]
mod tests;

use bytes::Bytes;
use super::checksum::Ipv4Checksum;
use crate::{
    fail::Fail,
    protocols::ethernet,
};
use byteorder::{
    NetworkEndian,
    WriteBytesExt,
    ByteOrder,
};
use header::{
    DEFAULT_IPV4_TTL,
    IPV4_IHL_NO_OPTIONS,
    IPV4_VERSION,
};
use std::{
    convert::TryFrom,
    convert::TryInto,
    io::Write,
    net::Ipv4Addr,
};
use num_traits::FromPrimitive;
pub use header::{
    Ipv4Header,
    Ipv4HeaderMut,
    Ipv4Protocol,
    IPV4_HEADER_SIZE,
};

#[repr(u8)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum Ipv4Protocol2 {
    Icmpv4 = 0x01,
    Tcp = 0x06,
    Udp = 0x11,
}

impl TryFrom<u8> for Ipv4Protocol2 {
    type Error = Fail;

    fn try_from(n: u8) -> Result<Self, Fail> {
        match FromPrimitive::from_u8(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported { details: "Unsupported IPv4 protocol" }),
        }
    }
}

pub const IPV4_HEADER2_SIZE: usize = 20;

pub struct Ipv4Header2 {
    // [ version 4 bits ] [ IHL 4 bits ]
    pub version: u8,
    pub ihl: u8,

    // [ DSCP 6 bits ] [ ECN 2 bits ]
    pub dscp: u8,
    pub ecn: u8,

    pub total_length: u16,
    pub identification: u16,

    // [ flags 3 bits ] [ fragment offset 13 bits ]
    pub flags: u8,
    pub fragment_offset: u16,

    pub time_to_live: u8,
    pub protocol: Ipv4Protocol2,

    // We omit the header checksum since it's checked when parsing and computed when serializing.
    // header_checksum: u16,

    pub src_addr: Ipv4Addr,
    pub dst_addr: Ipv4Addr,
}

fn ipv4_checksum(buf: &[u8]) -> u16 {
    let buf: &[u8; IPV4_HEADER2_SIZE] = buf.try_into().expect("Invalid header size");
    let mut state = 0xffffu32;
    for i in 0..5 {
        state += NetworkEndian::read_u16(&buf[2*i..2*i + 1]) as u32;
    }
    // Skip the 5th u16 since octets 10-12 are the header checksum, whose value should be zero when
    // computing a checksum.
    for i in 6..10 {
        state += NetworkEndian::read_u16(&buf[2*i..2*i + 1]) as u32;
    }
    while state > 0xffff {
        state -= 0xffff;
    }
    !state as u16
}

impl Ipv4Header2 {
    pub fn parse(mut buf: Bytes) -> Result<(Self, Bytes), Fail> {
        if buf.len() < IPV4_HEADER2_SIZE {
            return Err(Fail::Malformed { details: "Datagram too small" });
        }
        let payload_buf = buf.split_off(IPV4_HEADER2_SIZE);

        let version = buf[0] >> 4;
        if version != IPV4_VERSION {
            return Err(Fail::Unsupported { details: "Unsupported IP version" });
        }

        let ihl = buf[0] & 0xF;
        if ihl < IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Malformed { details: "IPv4 IHL is too small" });
        }
        if ihl > IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Unsupported { details: "IPv4 options are unsupported" });
        }

        let dscp = buf[1] >> 2;
        let ecn = buf[1] & 3;

        let total_length = NetworkEndian::read_u16(&buf[2..4]);
        if total_length as usize != IPV4_HEADER2_SIZE + payload_buf.len() {
            return Err(Fail::Malformed { details: "IPv4 TOTALLEN mismatch" });
        }

        let identification = NetworkEndian::read_u16(&buf[4..6]);
        let flags = (NetworkEndian::read_u16(&buf[6..8]) >> 13) as u8;

        let fragment_offset = NetworkEndian::read_u16(&buf[6..8]) & 0x1fff;
        if fragment_offset != 0 {
            return Err(Fail::Unsupported { details: "IPv4 fragmentation is unsupported" });
        }

        let time_to_live = buf[8];
        let protocol = Ipv4Protocol2::try_from(buf[9])?;

        let header_checksum = NetworkEndian::read_u16(&buf[10..12]);
        if header_checksum == 0xffff {
            return Err(Fail::Malformed { details: "IPv4 checksum is 0xFFFF" });
        }
        if header_checksum != ipv4_checksum(&buf[..]) {
            return Err(Fail::Malformed { details: "Invalid IPv4 checksum" });
        }

        let src_addr = Ipv4Addr::from(NetworkEndian::read_u32(&buf[12..16]));
        let dst_addr = Ipv4Addr::from(NetworkEndian::read_u32(&buf[16..20]));

        let header = Self {
            version,
            ihl,
            dscp,
            ecn,
            total_length,
            identification,
            flags,
            fragment_offset,
            time_to_live,
            protocol,
            src_addr,
            dst_addr,
        };
        Ok((header, payload_buf))
    }
}

#[derive(Clone, Copy)]
pub struct Ipv4Datagram<'a>(ethernet::Frame<'a>);

impl<'a> Ipv4Datagram<'a> {
    pub fn new_vec(text_len: usize) -> Vec<u8> {
        trace!("Ipv4DatagramMut::new({})", text_len);
        let requested_len = IPV4_HEADER_SIZE + text_len;
        let mut bytes = ethernet::Frame::new_vec(requested_len);
        let mut datagram = Ipv4DatagramMut::attach(bytes.as_mut());
        let mut ipv4_header = datagram.header();
        ipv4_header.version(IPV4_VERSION);
        ipv4_header.ihl(IPV4_IHL_NO_OPTIONS);
        ipv4_header.ttl(DEFAULT_IPV4_TTL);
        // the length of the datagram may not match what was requested due to
        // minimum frame size requirements; we set the `total_len` to be
        // smaller than the number of available bytes to distinguish padding
        // from actual payload (RFC 0894).
        let total_len = IPV4_HEADER_SIZE + text_len;
        ipv4_header.total_len(u16::try_from(total_len).unwrap());
        let mut frame_header = datagram.frame().header();
        frame_header.ether_type(ethernet::EtherType::Ipv4);
        bytes
    }

    pub fn attach(bytes: &'a [u8]) -> Result<Self, Fail> {
        Ok(Ipv4Datagram::try_from(ethernet::Frame::attach(bytes)?)?)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0.as_bytes()
    }

    pub fn header(&self) -> Ipv4Header<'_> {
        Ipv4Header::new(&self.0.text()[..IPV4_HEADER_SIZE])
    }

    #[allow(dead_code)]
    pub fn frame(&self) -> &ethernet::Frame<'a> {
        &self.0
    }

    pub fn text(&self) -> &[u8] {
        let total_len = self.header().total_len();
        &self.0.text()[IPV4_HEADER_SIZE..total_len]
    }
}

impl<'a> TryFrom<ethernet::Frame<'a>> for Ipv4Datagram<'a> {
    type Error = Fail;

    fn try_from(frame: ethernet::Frame<'a>) -> Result<Self, Fail> {
        trace!("Ipv4Datagram::try_from(...)");
        if frame.header().ether_type()? != ethernet::EtherType::Ipv4 {
            return Err(Fail::TypeMismatch {
                details: "expected an IPv4 datagram",
            });
        }

        if frame.text().len() <= IPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "IPv4 datagram is too small to contain a complete header",
            });
        }

        let datagram = Ipv4Datagram(frame);
        let text_len = datagram.text().len();
        let header = datagram.header();
        if header.version() != IPV4_VERSION {
            return Err(Fail::Unsupported {
                details: "unsupported IPv4 version",
            });
        }

        if header.total_len() != text_len + IPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "IPv4 TOTALLEN mismatch",
            });
        }

        let ihl = header.ihl();
        if ihl < IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Malformed {
                details: "IPv4 IHL is too small",
            });
        }

        // we don't currently support IPv4 options.
        if ihl > IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Unsupported {
                details: "IPv4 options are not supported",
            });
        }

        // we don't currently support fragmented packets.
        if header.frag_offset() != 0 {
            return Err(Fail::Unsupported {
                details: "IPv4 fragmentation is not supported",
            });
        }

        // from _TCP/IP Illustrated_, Section 5.2.2:
        // > Note that for any nontrivial packet or header, the value
        // > of the Checksum field in the packet can never be FFFF.
        let checksum = header.checksum();
        if checksum == 0xffff {
            return Err(Fail::Malformed {
                details: "invalid IPv4 checksum",
            });
        }

        let should_be_zero = {
            let mut checksum = Ipv4Checksum::new();
            checksum.write_all(header.as_bytes()).unwrap();
            checksum.finish()
        };

        if checksum != 0 && should_be_zero != 0 {
            return Err(Fail::Malformed {
                details: "IPv4 checksum mismatch",
            });
        }

        let _ = header.protocol()?;
        Ok(datagram)
    }
}

pub struct Ipv4DatagramMut<'a>(ethernet::FrameMut<'a>);

impl<'a> Ipv4DatagramMut<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> Self {
        Ipv4DatagramMut(ethernet::FrameMut::attach(bytes))
    }

    pub fn header(&mut self) -> Ipv4HeaderMut<'_> {
        Ipv4HeaderMut::new(&mut self.0.text()[..IPV4_HEADER_SIZE])
    }

    pub fn frame(&mut self) -> &mut ethernet::FrameMut<'a> {
        &mut self.0
    }

    pub fn text(&mut self) -> &mut [u8] {
        &mut self.0.text()[IPV4_HEADER_SIZE..]
    }

    pub fn unmut(&self) -> Ipv4Datagram<'_> {
        Ipv4Datagram(self.0.unmut())
    }

    pub fn write_checksum(mut self) {
        let mut checksum = Ipv4Checksum::new();
        let mut ipv4_header = self.header();
        checksum.write_all(&ipv4_header.as_bytes()[..10]).unwrap();
        checksum.write_u16::<NetworkEndian>(0u16).unwrap();
        checksum.write_all(&ipv4_header.as_bytes()[12..]).unwrap();
        ipv4_header.checksum(checksum.finish());
    }

    pub fn seal(mut self) -> Result<Ipv4Datagram<'a>, Fail> {
        trace!("Ipv4DatagramMut::seal()");
        let mut checksum = Ipv4Checksum::new();
        let mut ipv4_header = self.header();
        checksum.write_all(&ipv4_header.as_bytes()[..10]).unwrap();
        checksum.write_u16::<NetworkEndian>(0u16).unwrap();
        checksum.write_all(&ipv4_header.as_bytes()[12..]).unwrap();
        ipv4_header.checksum(checksum.finish());
        Ok(Ipv4Datagram::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<ethernet::FrameMut<'a>> for Ipv4DatagramMut<'a> {
    fn from(frame: ethernet::FrameMut<'a>) -> Self {
        Ipv4DatagramMut(frame)
    }
}
