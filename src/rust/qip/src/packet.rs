use crate::prelude::*;
use etherparse::{Ethernet2Header, Ethernet2HeaderSlice};
use fail::EtherParseError;

#[derive(From)]
pub struct Packet {
    bytes: Vec<u8>,
}

impl Into<Vec<u8>> for Packet {
    fn into(self) -> Vec<u8> {
        self.bytes
    }
}

impl Packet {
    pub fn parse_ether2_header(&self) -> Result<Ethernet2Header> {
        let sliced = Ethernet2HeaderSlice::from_slice(&self.bytes)
            .map_err(|e| EtherParseError::ReadError(e))?
            .slice();
        // `Ethernet2HeaderSlice.from_slice()` should ensure that there's nothing left over (second tuple component).
        let (header, extra) = Ethernet2Header::read_from_slice(sliced)
            .map_err(|e| EtherParseError::ReadError(e))?;
        assert_eq!(0, extra.len());
        Ok(header)
    }

    pub fn bytes<'a>(&'a self) -> &'a [u8] {
        &self.bytes
    }
}
