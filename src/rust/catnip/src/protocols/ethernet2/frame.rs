use super::header::Ethernet2Header;
use crate::prelude::*;
use std::{cmp::max, convert::TryFrom, io::Cursor};

// minimum paylod size is 46 bytes as described at [this wikipedia article](https://en.wikipedia.org/wiki/Ethernet_frame).
pub static MIN_PAYLOAD_SIZE: usize = 46;

pub struct Ethernet2Frame {
    bytes: Vec<u8>,
}

impl Ethernet2Frame {
    pub fn new(payload_sz: usize) -> Self {
        let payload_sz = max(payload_sz, MIN_PAYLOAD_SIZE);
        let packet_len = payload_sz + Ethernet2Header::size();
        Ethernet2Frame {
            bytes: vec![0u8; packet_len],
        }
    }

    pub fn read_header(&self) -> Result<Ethernet2Header> {
        let bytes = &self.bytes[..Ethernet2Header::size()];
        Ok(Ethernet2Header::read(&mut Cursor::new(&bytes))?)
    }

    pub fn write_header(&mut self, header: Ethernet2Header) -> Result<()> {
        let mut bytes = &mut self.bytes[..Ethernet2Header::size()];
        Ok(header.write(&mut bytes)?)
    }

    pub fn payload(&self) -> &[u8] {
        &self.bytes[Ethernet2Header::size()..]
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[Ethernet2Header::size()..]
    }
}

impl TryFrom<Vec<u8>> for Ethernet2Frame {
    type Error = Fail;

    fn try_from(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < MIN_PAYLOAD_SIZE + Ethernet2Header::size() {
            return Err(Fail::Malformed {});
        }

        Ok(Ethernet2Frame { bytes })
    }
}

impl Into<Vec<u8>> for Ethernet2Frame {
    fn into(self) -> Vec<u8> {
        self.bytes
    }
}
