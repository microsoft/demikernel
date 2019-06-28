mod header;

pub use header::{TcpHeader, TcpHeaderMut, MIN_TCP_HEADER_SIZE};

use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct TcpSegment<'a> {
    ipv4: ipv4::Datagram<'a>,
    header_len: usize,
}

impl<'a> TcpSegment<'a> {
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

impl<'a> TryFrom<ipv4::Datagram<'a>> for TcpSegment<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Tcp);
        let payload_len = ipv4_datagram.payload().len();
        if payload_len < MIN_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "TCP segment is too small to contain a complete \
                          header",
            });
        }

        let prefix =
            TcpHeader::new(&ipv4_datagram.payload()[..MIN_TCP_HEADER_SIZE]);
        let header_len = prefix.header_len();
        if payload_len < header_len {
            return Err(Fail::Malformed {
                details: "TCP HEADERLEN mismatch",
            });
        }

        Ok(TcpSegment {
            ipv4: ipv4_datagram,
            header_len,
        })
    }
}

// todo: need to determine how we will support TCP options.
pub struct TcpSegmentMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> TcpSegmentMut<'a> {
    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(TcpSegmentMut(ipv4::DatagramMut::from_bytes(bytes)?))
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

    pub fn unmut(self) /* -> TcpSegment<'a> */
    {
        unimplemented!()
    }

    pub fn seal(self) -> Result<TcpSegment<'a>> {
        trace!("TcpSegmentMut::seal()");
        let ipv4_datagram = {
            let mut ipv4 = self.0;
            let mut header = ipv4.header();
            header.protocol(ipv4::Protocol::Tcp);
            ipv4.seal()?
        };

        Ok(TcpSegment::try_from(ipv4_datagram)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for TcpSegmentMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        TcpSegmentMut(ipv4_datagram)
    }
}
