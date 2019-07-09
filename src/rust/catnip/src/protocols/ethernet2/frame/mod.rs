mod header;

use crate::prelude::*;
use std::cmp::max;

pub use header::{
    EtherType, Ethernet2Header, Ethernet2HeaderMut, ETHERNET2_HEADER_SIZE,
};

// minimum paylod size is 46 bytes as described at [this wikipedia article](https://en.wikipedia.org/wiki/Ethernet_frame).
pub static MIN_PAYLOAD_SIZE: usize = 46;

pub struct Ethernet2Frame<'a>(&'a [u8]);

impl<'a> Ethernet2Frame<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
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

    fn validate_buffer_length(len: usize) -> Result<()> {
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
    pub fn new_bytes(text_sz: usize) -> Vec<u8> {
        let text_sz = max(text_sz, MIN_PAYLOAD_SIZE);
        vec![0u8; text_sz + ETHERNET2_HEADER_SIZE]
    }

    pub fn from_bytes(bytes: &'a mut [u8]) -> Self {
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

    pub fn seal(self) -> Result<Ethernet2Frame<'a>> {
        trace!("Ethernet2FrameMut::seal()");
        Ok(Ethernet2Frame::from_bytes(self.0)?)
    }
}
