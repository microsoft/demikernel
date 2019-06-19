mod header;

pub use header::{TcpHeader, TcpHeaderMut, TCP_HEADER_SIZE};

use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct TcpDatagram<'a>(ipv4::Datagram<'a>);

impl<'a> TcpDatagram<'a> {
    pub fn header(&self) -> TcpHeader<'_> {
        TcpHeader::new(&self.0.payload()[..TCP_HEADER_SIZE])
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload()[TCP_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for TcpDatagram<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Tcp);
        if ipv4_datagram.payload().is_empty() {
            return Err(Fail::Malformed {});
        }

        Ok(TcpDatagram(ipv4_datagram))
    }
}

pub struct TcpDatagramMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> TcpDatagramMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(TcpDatagramMut(ipv4::DatagramMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> TcpHeaderMut<'_> {
        TcpHeaderMut::new(&mut self.0.payload()[..TCP_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn payload(&mut self) -> &mut [u8] {
        &mut self.0.payload()[TCP_HEADER_SIZE..]
    }

    #[allow(dead_code)]
    pub fn unmut(self) -> TcpDatagram<'a> {
        TcpDatagram(self.0.unmut())
    }

    pub fn seal(self) -> Result<TcpDatagram<'a>> {
        let ipv4_datagram = {
            let mut ipv4 = self.0;
            let mut header = ipv4.header();
            header.protocol(ipv4::Protocol::Tcp);
            ipv4.seal()?
        };

        Ok(TcpDatagram(ipv4_datagram))
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for TcpDatagramMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        TcpDatagramMut(ipv4_datagram)
    }
}
