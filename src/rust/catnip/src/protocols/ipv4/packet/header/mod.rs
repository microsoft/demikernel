#![allow(dead_code)]

#[cfg(test)]
mod tests;

pub mod checksum;

use crate::prelude::*;
use byteorder::{ByteOrder, NetworkEndian};
use num_traits::FromPrimitive;
use std::{convert::TryFrom, net::Ipv4Addr};

pub const IPV4_HEADER_SIZE: usize = 20;
// todo: need citation
pub const DEFAULT_IPV4_TTL: u8 = 64;
pub const IPV4_IHL_NO_OPTIONS: u8 = 5;
pub const IPV4_VERSION: u8 = 4;

#[repr(u8)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum Ipv4Protocol {
    Udp = 0x11,
}

impl TryFrom<u8> for Ipv4Protocol {
    type Error = Fail;

    fn try_from(n: u8) -> Result<Self> {
        match FromPrimitive::from_u8(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {}),
        }
    }
}

impl Into<u8> for Ipv4Protocol {
    fn into(self) -> u8 {
        self as u8
    }
}

pub struct Ipv4Header<'a>(&'a [u8]);

impl<'a> Ipv4Header<'a> {
    pub fn new(bytes: &'a [u8]) -> Ipv4Header<'a> {
        assert!(bytes.len() == IPV4_HEADER_SIZE);
        Ipv4Header(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0
    }

    pub fn version(&self) -> u8 {
        let n = self.0[0];
        n >> 4
    }

    pub fn ihl(&self) -> u8 {
        let n = self.0[0];
        n & 0xf
    }

    pub fn dscp(&self) -> u8 {
        let n = self.0[1];
        n >> 2
    }

    pub fn ecn(&self) -> u8 {
        let n = self.0[1];
        n & 3
    }

    pub fn total_len(&self) -> usize {
        usize::from(NetworkEndian::read_u16(&self.0[2..4]))
    }

    pub fn id(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[4..6])
    }

    pub fn flags(&self) -> u16 {
        let n = NetworkEndian::read_u16(&self.0[6..8]);
        n >> 13
    }

    pub fn frag_offset(&self) -> u16 {
        let n = NetworkEndian::read_u16(&self.0[6..8]);
        n & 0x1fff
    }

    pub fn ttl(&self) -> u8 {
        self.0[8]
    }

    pub fn protocol(&self) -> Result<Ipv4Protocol> {
        Ok(Ipv4Protocol::try_from(self.0[9])?)
    }

    pub fn checksum(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[10..12])
    }

    pub fn src_addr(&self) -> Ipv4Addr {
        Ipv4Addr::from(NetworkEndian::read_u32(&self.0[12..16]))
    }

    pub fn dest_addr(&self) -> Ipv4Addr {
        Ipv4Addr::from(NetworkEndian::read_u32(&self.0[16..20]))
    }
}

pub struct Ipv4HeaderMut<'a>(&'a mut [u8]);

impl<'a> Ipv4HeaderMut<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Ipv4HeaderMut<'a> {
        assert!(bytes.len() == IPV4_HEADER_SIZE);
        Ipv4HeaderMut(bytes)
    }

    pub fn as_bytes(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn version(&mut self, value: u8) {
        assert!(value <= 0xf);
        let n = self.0[0];
        self.0[0] = (n & 0x0f) | (value << 4);
    }

    pub fn ihl(&mut self, value: u8) {
        assert!(value <= 0xf);
        let n = self.0[0];
        self.0[0] = (n & 0xf0) | value;
    }

    pub fn dscp(&mut self, value: u8) {
        assert!(value <= 0x3f);
        let n = self.0[1];
        self.0[1] = (n & 0x3) | (value << 2);
    }

    pub fn ecn(&mut self, value: u8) {
        assert!(value <= 0x3);
        let n = self.0[1];
        self.0[1] = (n & 0xfc) | value;
    }

    pub fn total_len(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[2..4], value)
    }

    pub fn id(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[4..6], value)
    }

    pub fn flags(&mut self, value: u16) {
        assert!(value <= 0x7);
        let n = NetworkEndian::read_u16(&self.0[6..8]);
        NetworkEndian::write_u16(
            &mut self.0[6..8],
            (n & 0x1fff) | (value << 13),
        )
    }

    pub fn frag_offset(&mut self, value: u16) {
        assert!(value <= 0x1fff);
        let n = NetworkEndian::read_u16(&self.0[6..8]);
        NetworkEndian::write_u16(&mut self.0[6..8], (n & 0xe000) | value)
    }

    pub fn ttl(&mut self, value: u8) {
        self.0[8] = value;
    }

    pub fn protocol(&mut self, value: Ipv4Protocol) {
        self.0[9] = value.into();
    }

    pub fn checksum(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[10..12], value);
    }

    pub fn src_addr(&mut self, value: Ipv4Addr) {
        NetworkEndian::write_u32(&mut self.0[12..16], value.into());
    }

    pub fn dest_addr(&mut self, value: Ipv4Addr) {
        NetworkEndian::write_u32(&mut self.0[16..20], value.into());
    }
}
