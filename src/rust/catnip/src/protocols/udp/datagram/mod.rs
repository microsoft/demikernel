mod header;

pub use header::{UdpHeader, UdpHeaderMut, UDP_HEADER_SIZE};

use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct UdpDatagram<'a>(ipv4::Datagram<'a>);

impl<'a> UdpDatagram<'a> {
    pub fn header(&self) -> UdpHeader<'_> {
        UdpHeader::new(&self.0.payload()[..UDP_HEADER_SIZE])
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload()[UDP_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for UdpDatagram<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Udp);
        if ipv4_datagram.payload().is_empty() {
            return Err(Fail::Malformed {});
        }

        Ok(UdpDatagram(ipv4_datagram))
    }
}

pub struct UdpDatagramMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> UdpDatagramMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(UdpDatagramMut(ipv4::DatagramMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> UdpHeaderMut<'_> {
        UdpHeaderMut::new(&mut self.0.payload()[..UDP_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn payload(&mut self) -> &mut [u8] {
        &mut self.0.payload()[UDP_HEADER_SIZE..]
    }

    #[allow(dead_code)]
    pub fn unmut(self) -> UdpDatagram<'a> {
        UdpDatagram(self.0.unmut())
    }

    pub fn seal(self) -> Result<UdpDatagram<'a>> {
        let ipv4_datagram = {
            let mut ipv4 = self.0;
            let mut header = ipv4.header();
            header.protocol(ipv4::Protocol::Udp);
            ipv4.seal()?
        };

        Ok(UdpDatagram(ipv4_datagram))
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for UdpDatagramMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        UdpDatagramMut(ipv4_datagram)
    }
}
