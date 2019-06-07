use super::header::{Ipv4Header, IPV4_HEADER_SIZE};
use crate::{prelude::*, protocols::ethernet2};
use std::convert::TryFrom;

pub struct Ipv4Packet(ethernet2::Frame);

impl Ipv4Packet {
    pub fn frame(&self) -> &ethernet2::Frame {
        &self.0
    }

    pub fn header(&self) -> Ipv4Header<&[u8]> {
        Ipv4Header(&self.0.payload()[..IPV4_HEADER_SIZE])
    }

    pub fn header_mut(&mut self) -> Ipv4Header<&mut [u8]> {
        Ipv4Header(&mut self.0.payload_mut()[..IPV4_HEADER_SIZE])
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload()[IPV4_HEADER_SIZE..]
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.0.payload_mut()[IPV4_HEADER_SIZE..]
    }
}

impl TryFrom<ethernet2::Frame> for Ipv4Packet {
    type Error = Fail;

    fn try_from(mut frame: ethernet2::Frame) -> Result<Self> {
        assert_eq!(frame.header().ether_type, ethernet2::EtherType::Ipv4);
        Ipv4Header::validate(frame.payload_mut())?;
        let total_len = Ipv4Header(frame.payload_mut()).get_total_len();
        if total_len as usize != frame.payload().len() {
            return Err(Fail::Malformed {});
        }

        Ok(Ipv4Packet(frame))
    }
}
