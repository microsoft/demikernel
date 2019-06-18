use super::header::{UdpHeader, UdpHeaderMut, UDP_HEADER_SIZE};
use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct UdpPacket<'a>(ipv4::Packet<'a>);

impl<'a> UdpPacket<'a> {
    pub fn header(&self) -> UdpHeader<'_> {
        UdpHeader::new(&self.0.payload()[..UDP_HEADER_SIZE])
    }

    pub fn ipv4(&self) -> &ipv4::Packet<'a> {
        &self.0
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload()[UDP_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ipv4::Packet<'a>> for UdpPacket<'a> {
    type Error = Fail;

    fn try_from(ipv4_packet: ipv4::Packet<'a>) -> Result<Self> {
        assert_eq!(ipv4_packet.header().protocol()?, ipv4::Protocol::Udp);
        if ipv4_packet.payload().is_empty() {
            return Err(Fail::Malformed {});
        }

        Ok(UdpPacket(ipv4_packet))
    }
}

pub struct UdpPacketMut<'a>(ipv4::PacketMut<'a>);

impl<'a> UdpPacketMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(UdpPacketMut(ipv4::PacketMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> UdpHeaderMut<'_> {
        UdpHeaderMut::new(&mut self.0.payload()[..UDP_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::PacketMut<'a> {
        &mut self.0
    }

    pub fn payload(&mut self) -> &mut [u8] {
        &mut self.0.payload()[UDP_HEADER_SIZE..]
    }

    #[allow(dead_code)]
    pub fn unmut(self) -> UdpPacket<'a> {
        UdpPacket(self.0.unmut())
    }

    pub fn seal(self) -> Result<UdpPacket<'a>> {
        let ipv4_packet = {
            let mut ipv4 = self.0;
            let mut header = ipv4.header();
            header.protocol(ipv4::Protocol::Udp);
            ipv4.seal()?
        };

        Ok(UdpPacket(ipv4_packet))
    }
}

impl<'a> From<ipv4::PacketMut<'a>> for UdpPacketMut<'a> {
    fn from(ipv4_packet: ipv4::PacketMut<'a>) -> Self {
        UdpPacketMut(ipv4_packet)
    }
}
