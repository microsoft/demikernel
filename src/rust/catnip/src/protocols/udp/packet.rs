use super::header::UdpHeader;
use crate::{prelude::*, protocols::ipv4};
use std::io::Cursor;

pub struct UdpPacket {
    ipv4: ipv4::Packet,
}

impl UdpPacket {
    pub fn new(payload_sz: usize) -> Self {
        UdpPacket {
            ipv4: ipv4::Packet::new(payload_sz + UdpHeader::size()),
        }
    }

    pub fn ipv4(&self) -> &ipv4::Packet {
        &self.ipv4
    }

    pub fn ipv4_mut(&mut self) -> &mut ipv4::Packet {
        &mut self.ipv4
    }

    pub fn read_header(&self) -> Result<UdpHeader> {
        let bytes = &self.ipv4().payload()[..UdpHeader::size()];
        Ok(UdpHeader::read(&mut Cursor::new(bytes))?)
    }

    pub fn write_header(&mut self, header: UdpHeader) -> Result<()> {
        let payload_len = self.ipv4().payload().len();
        let payload = self.ipv4_mut().payload_mut();
        let mut bytes = &mut payload[..UdpHeader::size()];
        Ok(header.write(&mut bytes, payload_len)?)
    }

    pub fn payload(&mut self) -> &[u8] {
        &self.ipv4().payload()[UdpHeader::size()..]
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.ipv4_mut().payload_mut()[UdpHeader::size()..]
    }
}

impl From<ipv4::Packet> for UdpPacket {
    fn from(ipv4: ipv4::Packet) -> Self {
        UdpPacket { ipv4 }
    }
}

impl Into<Vec<u8>> for UdpPacket {
    fn into(self) -> Vec<u8> {
        self.ipv4.into()
    }
}
