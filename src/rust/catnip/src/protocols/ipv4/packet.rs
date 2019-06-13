use super::header::Ipv4Header;
use crate::{prelude::*, protocols::ethernet2};
use std::io::Cursor;

pub struct Ipv4Packet {
    frame: ethernet2::Frame,
}

impl Ipv4Packet {
    pub fn new(payload_sz: usize) -> Self {
        Ipv4Packet {
            frame: ethernet2::Frame::new(payload_sz + Ipv4Header::size()),
        }
    }

    pub fn frame(&self) -> &ethernet2::Frame {
        &self.frame
    }

    pub fn frame_mut(&mut self) -> &mut ethernet2::Frame {
        &mut self.frame
    }

    pub fn read_header(&self) -> Result<Ipv4Header> {
        let payload = self.payload();
        let bytes = &payload[..Ipv4Header::size()];
        Ok(Ipv4Header::read(&mut Cursor::new(bytes), payload.len())?)
    }

    pub fn write_header(&mut self, header: Ipv4Header) -> Result<()> {
        let payload_len = self.payload().len();
        let mut bytes =
            &mut self.frame_mut().payload_mut()[..Ipv4Header::size()];
        Ok(header.write(&mut bytes, payload_len)?)
    }

    pub fn payload(&self) -> &[u8] {
        &self.frame().payload()[Ipv4Header::size()..]
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.frame_mut().payload_mut()[Ipv4Header::size()..]
    }
}

impl From<ethernet2::Frame> for Ipv4Packet {
    fn from(frame: ethernet2::Frame) -> Self {
        Ipv4Packet { frame }
    }
}

impl Into<Vec<u8>> for Ipv4Packet {
    fn into(self) -> Vec<u8> {
        self.frame.into()
    }
}

