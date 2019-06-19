mod header;

pub use header::{TcpHeader, TcpHeaderMut, MIN_TCP_HEADER_SIZE};

use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct TcpDatagram<'a> {
    ipv4: ipv4::Datagram<'a>,
    header_len: usize,
}

impl<'a> TcpDatagram<'a> {
    pub fn header(&self) -> TcpHeader<'_> {
        TcpHeader::new(&self.ipv4.payload()[..self.header_len])
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.ipv4
    }

    pub fn payload(&self) -> &[u8] {
        &self.ipv4.payload()[self.header_len..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for TcpDatagram<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Tcp);
        let payload_len = ipv4_datagram.payload().len();
        if payload_len < MIN_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {});
        }

        let prefix = TcpHeader::new(&ipv4_datagram.payload()[..MIN_TCP_HEADER_SIZE]);
        let header_len = prefix.header_len();
        if payload_len < header_len {
            return Err(Fail::Malformed {});
        }

        Ok(TcpDatagram { ipv4: ipv4_datagram, header_len })
    }
}

// todo: need to determine how we will support TCP options.
pub struct TcpDatagramMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> TcpDatagramMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(TcpDatagramMut(ipv4::DatagramMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> TcpHeaderMut<'_> {
        TcpHeaderMut::new(&mut self.0.payload()[..MIN_TCP_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn payload(&mut self) -> &mut [u8] {
        &mut self.0.payload()[MIN_TCP_HEADER_SIZE..]
    }

    #[allow(dead_code)]
    pub fn unmut(self) -> Result<TcpDatagram<'a>> {
        Ok(TcpDatagram::try_from(self.0.unmut()?)?)
    }

    pub fn seal(self) -> Result<TcpDatagram<'a>> {
        let ipv4_datagram = {
            let mut ipv4 = self.0;
            let mut header = ipv4.header();
            header.protocol(ipv4::Protocol::Tcp);
            ipv4.seal()?
        };

        Ok(TcpDatagram::try_from(ipv4_datagram)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for TcpDatagramMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        TcpDatagramMut(ipv4_datagram)
    }
}
