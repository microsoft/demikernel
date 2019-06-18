use super::header::{
    Ethernet2Header, Ethernet2HeaderMut, ETHERNET2_HEADER_SIZE,
};
use crate::prelude::*;

// minimum paylod size is 46 bytes as described at [this wikipedia article](https://en.wikipedia.org/wiki/Ethernet_frame).
pub static MIN_PAYLOAD_SIZE: usize = 46;

pub struct Ethernet2Frame<'a>(&'a [u8]);

impl<'a> Ethernet2Frame<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        if bytes.as_ref().len() < ETHERNET2_HEADER_SIZE + MIN_PAYLOAD_SIZE {
            return Err(Fail::Malformed {});
        }

        Ok(Ethernet2Frame(bytes))
    }

    #[allow(dead_code)]
    pub fn as_bytes(&self) -> &[u8] {
        self.0
    }

    pub fn header(&self) -> Ethernet2Header<'_> {
        Ethernet2Header::new(&self.0[..ETHERNET2_HEADER_SIZE])
    }

    pub fn payload(&self) -> &[u8] {
        &self.0[ETHERNET2_HEADER_SIZE..]
    }
}

pub struct Ethernet2FrameMut<'a>(&'a mut [u8]);

impl<'a> Ethernet2FrameMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        let _ = Ethernet2Frame::from_bytes(&bytes)?;
        Ok(Ethernet2FrameMut(bytes))
    }

    #[allow(dead_code)]
    pub fn as_bytes(&mut self) -> &mut [u8] {
        self.0
    }

    pub fn header(&mut self) -> Ethernet2HeaderMut<'_> {
        Ethernet2HeaderMut::new(&mut self.0[..ETHERNET2_HEADER_SIZE])
    }

    pub fn payload(&mut self) -> &mut [u8] {
        &mut self.0[ETHERNET2_HEADER_SIZE..]
    }

    pub fn unmut(self) -> Ethernet2Frame<'a> {
        Ethernet2Frame(self.0)
    }
}
