mod header;

pub use header::{UdpHeader, UdpHeaderMut, UDP_HEADER_SIZE};

use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct UdpDatagram<'a>(ipv4::Datagram<'a>);

impl<'a> UdpDatagram<'a> {
    pub fn new(text_sz: usize) -> Vec<u8> {
        let mut bytes = ipv4::Datagram::new(text_sz + UDP_HEADER_SIZE);
        let mut datagram = UdpDatagramMut::attach(bytes.as_mut());
        datagram.ipv4().header().protocol(ipv4::Protocol::Udp);
        bytes
    }

    pub fn attach(bytes: &'a [u8]) -> Result<Self> {
        Ok(UdpDatagram::try_from(ipv4::Datagram::attach(bytes)?)?)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0.as_bytes()
    }

    pub fn header(&self) -> UdpHeader<'_> {
        UdpHeader::new(&self.0.text()[..UDP_HEADER_SIZE])
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn text(&self) -> &[u8] {
        &self.0.text()[UDP_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for UdpDatagram<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Udp);
        if ipv4_datagram.text().len() < UDP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "UDP datagram is too small to contain a complete \
                          header",
            });
        }

        Ok(UdpDatagram(ipv4_datagram))
    }
}

impl<'a> Into<ipv4::Datagram<'a>> for UdpDatagram<'a> {
    fn into(self) -> ipv4::Datagram<'a> {
        self.0
    }
}

pub struct UdpDatagramMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> UdpDatagramMut<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> Self {
        UdpDatagramMut(ipv4::DatagramMut::attach(bytes))
    }

    pub fn header(&mut self) -> UdpHeaderMut<'_> {
        UdpHeaderMut::new(&mut self.0.text()[..UDP_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn text(&mut self) -> &mut [u8] {
        &mut self.0.text()[UDP_HEADER_SIZE..]
    }

    pub fn unmut(&self) -> UdpDatagram<'_> {
        UdpDatagram(self.0.unmut())
    }

    pub fn seal(self) -> Result<UdpDatagram<'a>> {
        trace!("UdpDatagramMut::seal()");
        Ok(UdpDatagram::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for UdpDatagramMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        UdpDatagramMut(ipv4_datagram)
    }
}
