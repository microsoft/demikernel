// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![allow(dead_code)]

#[cfg(test)]
mod tests;

mod options;

use crate::{
    fail::Fail,
    protocols::ip,
};
use byteorder::{
    ByteOrder,
    NetworkEndian,
};
use std::{
    cmp::{
        max,
        min,
    },
    convert::TryFrom,
    num::Wrapping,
};

pub use options::{
    TcpSegmentOptions,
    DEFAULT_MSS,
    FALLBACK_MSS,
    MAX_MSS,
    MIN_MSS,
};

pub const MIN_TCP_HEADER_SIZE: usize = 20;
pub const MAX_TCP_HEADER_SIZE: usize = 60;

pub struct TcpHeaderDecoder<'a>(&'a [u8]);

impl<'a> TcpHeaderDecoder<'a> {
    pub fn attach(bytes: &'a [u8]) -> Result<TcpHeaderDecoder<'a>, Fail> {
        if bytes.len() < MIN_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "buffer is too small for minimum TCP header",
            });
        }

        let header = TcpHeaderDecoder(bytes);
        let header_len = header.header_len();
        if header_len > MAX_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "TCP header length is too large",
            });
        }

        if header_len > MIN_TCP_HEADER_SIZE {
            let options = TcpSegmentOptions::parse(&bytes[MIN_TCP_HEADER_SIZE..])?;
            if header_len != options.header_length() {
                return Err(Fail::Malformed {
                    details: "TCP header length mismatch",
                });
            }
            Ok(TcpHeaderDecoder(&bytes[..header_len]))
        } else {
            Ok(header)
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..self.header_len()]
    }

    pub fn src_port(&self) -> Option<ip::Port> {
        match ip::Port::try_from(NetworkEndian::read_u16(&self.0[..2])) {
            Ok(p) => Some(p),
            _ => None,
        }
    }

    pub fn dest_port(&self) -> Option<ip::Port> {
        match ip::Port::try_from(NetworkEndian::read_u16(&self.0[2..4])) {
            Ok(p) => Some(p),
            _ => None,
        }
    }

    pub fn seq_num(&self) -> Wrapping<u32> {
        Wrapping(NetworkEndian::read_u32(&self.0[4..8]))
    }

    pub fn ack_num(&self) -> Wrapping<u32> {
        Wrapping(NetworkEndian::read_u32(&self.0[8..12]))
    }

    pub fn header_len(&self) -> usize {
        let n = usize::from(self.0[12] >> 4);
        n * 4
    }

    pub fn ns(&self) -> bool {
        (self.0[12] & 1) != 0u8
    }

    pub fn cwr(&self) -> bool {
        (self.0[13] & (1 << 7)) != 0u8
    }

    pub fn ece(&self) -> bool {
        (self.0[13] & (1 << 6)) != 0u8
    }

    pub fn urg(&self) -> bool {
        (self.0[13] & (1 << 5)) != 0u8
    }

    pub fn ack(&self) -> bool {
        (self.0[13] & (1 << 4)) != 0u8
    }

    pub fn psh(&self) -> bool {
        (self.0[13] & (1 << 3)) != 0u8
    }

    pub fn rst(&self) -> bool {
        (self.0[13] & (1 << 2)) != 0u8
    }

    pub fn syn(&self) -> bool {
        (self.0[13] & (1 << 1)) != 0u8
    }

    pub fn fin(&self) -> bool {
        (self.0[13] & 1) != 0u8
    }

    pub fn window_size(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[14..16])
    }

    pub fn checksum(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[16..18])
    }

    pub fn urg_ptr(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[18..20])
    }

    pub fn options(&self) -> TcpSegmentOptions {
        if self.header_len() == MIN_TCP_HEADER_SIZE {
            TcpSegmentOptions::new()
        } else {
            TcpSegmentOptions::parse(&self.0[MIN_TCP_HEADER_SIZE..]).unwrap()
        }
    }
}

pub struct TcpHeaderEncoder<'a>(&'a mut [u8]);

impl<'a> TcpHeaderEncoder<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> TcpHeaderEncoder<'a> {
        let len = min(bytes.len(), MAX_TCP_HEADER_SIZE + 1);
        TcpHeaderEncoder(&mut bytes[..len])
    }

    pub fn unmut(&self) -> TcpHeaderDecoder<'_> {
        TcpHeaderDecoder(self.0)
    }

    pub fn as_bytes(&mut self) -> &mut [u8] {
        let header_len = max(MIN_TCP_HEADER_SIZE, self.unmut().header_len());
        &mut self.0[..header_len]
    }

    pub fn src_port(&mut self, port: ip::Port) {
        NetworkEndian::write_u16(&mut self.0[..2], port.into())
    }

    pub fn dest_port(&mut self, port: ip::Port) {
        NetworkEndian::write_u16(&mut self.0[2..4], port.into())
    }

    pub fn seq_num(&mut self, value: Wrapping<u32>) {
        NetworkEndian::write_u32(&mut self.0[4..8], value.0)
    }

    pub fn ack_num(&mut self, value: Wrapping<u32>) {
        NetworkEndian::write_u32(&mut self.0[8..12], value.0)
    }

    fn header_len(&mut self, value: usize) {
        // from [wikipedia](https://en.wikipedia.org/wiki/Transmission_Control_Protocol#TCP_segment_structure)
        // > Specifies the size of the TCP header in 32-bit words. The
        // > minimum size header is 5 words and the maximum is 15 words thus
        // > giving the minimum size of 20 bytes and maximum of 60 bytes,
        // > allowing for up to 40 bytes of options in the header.
        assert!(value >= MIN_TCP_HEADER_SIZE);
        assert!(value <= MAX_TCP_HEADER_SIZE);

        let mut n = value / 4;
        if n * 4 != value {
            n += 1
        }

        let n = u8::try_from(n).unwrap();
        self.0[12] = (self.0[12] & 0xf) | (n << 4);
    }

    pub fn ns(&mut self, value: bool) {
        if value {
            self.0[12] |= 1u8;
        } else {
            self.0[12] &= !1u8;
        }
    }

    pub fn cwr(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 7;
        } else {
            self.0[13] &= !(1u8 << 7);
        }
    }

    pub fn ece(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 6;
        } else {
            self.0[13] &= !(1u8 << 6);
        }
    }

    pub fn urg(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 5;
        } else {
            self.0[13] &= !(1u8 << 5);
        }
    }

    pub fn ack(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 4;
        } else {
            self.0[13] &= !(1u8 << 4);
        }
    }

    pub fn psh(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 3;
        } else {
            self.0[13] &= !(1u8 << 3);
        }
    }

    pub fn rst(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 2;
        } else {
            self.0[13] &= !(1u8 << 2);
        }
    }

    pub fn syn(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8 << 1;
        } else {
            self.0[13] &= !(1u8 << 1);
        }
    }

    pub fn fin(&mut self, value: bool) {
        if value {
            self.0[13] |= 1u8;
        } else {
            self.0[13] &= !1u8;
        }
    }

    pub fn window_size(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[14..16], value);
    }

    pub fn checksum(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[16..18], value);
    }

    pub fn urg_ptr(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[18..20], value);
    }

    pub fn options(&mut self, options: TcpSegmentOptions) {
        let header_len = options.header_length();
        assert!(self.0.len() >= header_len);
        options.encode(&mut self.0[MIN_TCP_HEADER_SIZE..]);
        self.header_len(header_len);
    }
}
