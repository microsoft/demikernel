use super::{header::Ethernet2Header, MIN_PAYLOAD_SIZE};
use crate::prelude::*;
use std::{cmp::max, convert::TryFrom, io::Cursor};

#[derive(From)]
pub struct Ethernet2Frame {
    bytes: Vec<u8>,
    header: Ethernet2Header,
}

impl TryFrom<Vec<u8>> for Ethernet2Frame {
    type Error = Fail;

    fn try_from(bytes: Vec<u8>) -> Result<Ethernet2Frame> {
        if bytes.len() < MIN_PAYLOAD_SIZE {
            return Err(Fail::Malformed {});
        }

        let mut cursor = Cursor::new(&bytes);
        let header = Ethernet2Header::read(&mut cursor)?;
        assert_eq!(cursor.position() as usize, Ethernet2Header::size());

        Ok(Ethernet2Frame { bytes, header })
    }
}

impl Ethernet2Frame {
    pub fn new(
        payload_sz: usize,
        header: Ethernet2Header,
    ) -> Result<Ethernet2Frame> {
        let payload_sz = max(payload_sz, MIN_PAYLOAD_SIZE);
        let packet_len = payload_sz + Ethernet2Header::size();
        let mut bytes = vec![0u8; packet_len];
        header.write(&mut bytes)?;
        Ok(Ethernet2Frame { bytes, header })
    }

    pub fn header(&self) -> &Ethernet2Header {
        &self.header
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn payload(&self) -> &[u8] {
        &self.bytes[Ethernet2Header::size()..]
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[Ethernet2Header::size()..]
    }
}
