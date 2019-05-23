use super::header::Ethernet2Header;
use crate::prelude::*;
use std::{convert::TryFrom, io::Cursor};

#[derive(From)]
pub struct Ethernet2Frame {
    bytes: Vec<u8>,
    header: Ethernet2Header,
}

impl TryFrom<Vec<u8>> for Ethernet2Frame {
    type Error = Fail;

    fn try_from(bytes: Vec<u8>) -> Result<Ethernet2Frame> {
        let mut cursor = Cursor::new(&bytes);
        let header = Ethernet2Header::read(&mut cursor)?;
        assert_eq!(cursor.position() as usize, Ethernet2Header::size());

        Ok(Ethernet2Frame { bytes, header })
    }
}

impl Ethernet2Frame {
    pub fn header(&self) -> &Ethernet2Header {
        &self.header
    }

    pub fn payload(&self) -> &[u8] {
        &self.bytes[Ethernet2Header::size()..]
    }
}
