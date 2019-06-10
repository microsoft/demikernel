use super::header::{
    Ethernet2Header, Ethernet2HeaderMut, ETHERNET2_HEADER_SIZE,
};
use crate::prelude::*;

pub const MIN_PAYLOAD_SIZE: usize = 48;

pub struct Ethernet2Frame<'a>(&'a [u8]);

impl<'a> Ethernet2Frame<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Ethernet2Frame<'a>> {
        if bytes.len() < MIN_PAYLOAD_SIZE {
            return Err(Fail::Malformed {});
        }

        Ok(Ethernet2Frame(bytes))
    }

    pub fn bytes(&self) -> &[u8] {
        self.0
    }

    pub fn header(&self) -> Ethernet2Header<'a, &[u8]> {
        Ethernet2Header::new(&self.0)
    }

    pub fn payload(&self) -> &[u8] {
        &self.0[ETHERNET2_HEADER_SIZE..]
    }
}

pub struct Ethernet2FrameMut<'a>(&'a mut [u8]);

impl<'a> Ethernet2FrameMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Ethernet2FrameMut<'a>> {
        if bytes.len() < MIN_PAYLOAD_SIZE {
            return Err(Fail::Malformed {});
        }

        Ok(Ethernet2FrameMut(bytes))
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn header_mut(&mut self) -> Ethernet2HeaderMut<'a, &mut [u8]> {
        Ethernet2HeaderMut::new(&mut self.0)
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.0[ETHERNET2_HEADER_SIZE..]
    }
}
