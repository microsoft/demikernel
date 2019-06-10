use super::header::{UdpHeaderMut, UDP_HEADER_SIZE};
use crate::{
    prelude::*,
    protocols::{ethernet2, ipv4},
};

pub struct UdpPacketMut<'a>(ipv4::PacketMut<'a>);

impl<'a> UdpPacketMut<'a> {
    pub fn from_bytes(bytes: &mut [u8]) -> Result<UdpPacketMut> {
        Ok(UdpPacketMut(ipv4::PacketMut::from_bytes(bytes)?))
    }

    pub fn frame_mut(&mut self) -> &mut ethernet2::FrameMut<'a> {
        self.0.frame_mut()
    }

    pub fn ipv4_mut(&mut self) -> &mut ipv4::PacketMut<'a> {
        &mut self.0
    }

    pub fn header_mut(&mut self) -> UdpHeaderMut<&mut [u8]> {
        UdpHeaderMut(&mut self.0.payload_mut()[..UDP_HEADER_SIZE])
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.0.payload_mut()[UDP_HEADER_SIZE..]
    }
}
