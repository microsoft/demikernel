// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

use bytes::Bytes;
use byteorder::{ByteOrder, NetworkEndian};
use crate::fail::Fail;
use std::cmp::max;
use crate::protocols::ethernet::MacAddress;
use std::convert::TryFrom;
use num_traits::FromPrimitive;

pub use header::{
    EtherType,
    Ethernet2Header,
    Ethernet2HeaderMut,
    ETHERNET2_HEADER_SIZE,
};

#[repr(u16)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum EtherType2 {
    Arp = 0x806,
    Ipv4 = 0x800,
}

impl TryFrom<u16> for EtherType2 {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self, Fail> {
        match FromPrimitive::from_u16(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported { details: "Unsupported ETHERTYPE" }),
        }
    }
}

pub const ETHERNET2_HEADER2_SIZE: usize = 14;

pub struct Ethernet2Header2 {
    // Bytes 0..6
    pub dst_addr: MacAddress,
    // Bytes 6..12
    pub src_addr: MacAddress,
    // Bytes 12..14
    pub ether_type: EtherType2,
}

impl Ethernet2Header2 {
    pub fn parse(mut buf: Bytes) -> Result<(Self, Bytes), Fail> {
        if buf.len() < ETHERNET2_HEADER2_SIZE {
            return Err(Fail::Malformed { details: "Frame too small" });
        }
        let payload_buf = buf.split_off(ETHERNET2_HEADER2_SIZE);

        let dst_addr = MacAddress::from_bytes(&buf[0..6]);
        let src_addr = MacAddress::from_bytes(&buf[6..12]);
        let ether_type = EtherType2::try_from(NetworkEndian::read_u16(&buf[12..14]))?;
        let hdr = Self { dst_addr, src_addr, ether_type };
        Ok((hdr, payload_buf))
    }
}

// minimum paylod size is 46 bytes as described at [this wikipedia article](https://en.wikipedia.org/wiki/Ethernet_frame).
pub static MIN_PAYLOAD_SIZE: usize = 46;

#[derive(Clone, Copy)]
pub struct Ethernet2Frame<'a>(&'a [u8]);

impl<'a> Ethernet2Frame<'a> {
    pub fn new_vec(text_len: usize) -> Vec<u8> {
        trace!("Ethernet2Frame::new_vec({})", text_len);
        let text_len = max(text_len, MIN_PAYLOAD_SIZE);
        vec![0u8; text_len + ETHERNET2_HEADER_SIZE]
    }

    pub fn attach(bytes: &'a [u8]) -> Result<Self, Fail> {
        Ethernet2Frame::validate_buffer_length(bytes.len())?;

        let frame = Ethernet2Frame(bytes);
        let header = frame.header();
        if header.src_addr().is_nil() {
            return Err(Fail::Malformed {
                details: "source link address is nil",
            });
        }

        if header.dest_addr().is_nil() {
            return Err(Fail::Malformed {
                details: "destination link address is nil",
            });
        }

        Ok(frame)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0
    }

    pub fn header(&self) -> Ethernet2Header<'_> {
        Ethernet2Header::new(&self.0[..ETHERNET2_HEADER_SIZE])
    }

    pub fn text(&self) -> &[u8] {
        &self.0[ETHERNET2_HEADER_SIZE..]
    }

    fn validate_buffer_length(len: usize) -> Result<(), Fail> {
        if len < ETHERNET2_HEADER_SIZE + MIN_PAYLOAD_SIZE {
            Err(Fail::Malformed {
                details: "frame is shorter than the minimum length",
            })
        } else {
            Ok(())
        }
    }
}

pub struct Ethernet2FrameMut<'a>(&'a mut [u8]);

impl<'a> Ethernet2FrameMut<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> Self {
        Ethernet2Frame::validate_buffer_length(bytes.len())
            .expect("not enough bytes for a complete frame");
        Ethernet2FrameMut(bytes)
    }

    #[allow(dead_code)]
    pub fn as_bytes(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn header(&mut self) -> Ethernet2HeaderMut<'_> {
        Ethernet2HeaderMut::new(&mut self.0[..ETHERNET2_HEADER_SIZE])
    }

    pub fn text(&mut self) -> &mut [u8] {
        &mut self.0[ETHERNET2_HEADER_SIZE..]
    }

    pub fn unmut(&self) -> Ethernet2Frame<'_> {
        Ethernet2Frame(self.0)
    }

    pub fn seal(self) -> Result<Ethernet2Frame<'a>, Fail> {
        trace!("Ethernet2FrameMut::seal()");
        Ok(Ethernet2Frame::attach(self.0)?)
    }
}
