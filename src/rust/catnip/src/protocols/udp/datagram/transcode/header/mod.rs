#![allow(dead_code)]

#[cfg(test)]
mod tests;

use crate::{prelude::*, protocols::ip};
use byteorder::{ByteOrder, NetworkEndian};

pub const UDP_HEADER_SIZE: usize = 8;

pub struct UdpHeader<'a>(&'a [u8]);

impl<'a> UdpHeader<'a> {
    pub fn new(bytes: &'a [u8]) -> UdpHeader<'a> {
        assert!(bytes.len() == UDP_HEADER_SIZE);
        UdpHeader(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0
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

    pub fn length(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[4..6])
    }

    pub fn checksum(&self) -> u16 {
        NetworkEndian::read_u16(&self.0[6..8])
    }
}

pub struct UdpHeaderMut<'a>(&'a mut [u8]);

impl<'a> UdpHeaderMut<'a> {
    pub fn new(bytes: &'a mut [u8]) -> UdpHeaderMut<'a> {
        assert!(bytes.len() == UDP_HEADER_SIZE);
        UdpHeaderMut(bytes)
    }

    pub fn as_bytes(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn src_port(&mut self, port: ip::Port) {
        NetworkEndian::write_u16(&mut self.0[..2], port.into())
    }

    pub fn dest_port(&mut self, port: ip::Port) {
        NetworkEndian::write_u16(&mut self.0[2..4], port.into())
    }

    pub fn length(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[4..6], value)
    }

    pub fn checksum(&mut self, value: u16) {
        NetworkEndian::write_u16(&mut self.0[6..8], value)
    }
}
