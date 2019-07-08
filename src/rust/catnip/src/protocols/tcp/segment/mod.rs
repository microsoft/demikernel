mod header;

pub use header::{
    TcpHeader, TcpHeaderMut, MAX_TCP_HEADER_SIZE, MIN_TCP_HEADER_SIZE,
};

use crate::{prelude::*, protocols::ipv4};
use std::convert::TryFrom;

pub struct TcpSegment<'a>(ipv4::Datagram<'a>);

impl<'a> TcpSegment<'a> {
    pub fn header(&self) -> TcpHeader<'_> {
        // the header contents were validated when `try_from()` was called.
        TcpHeader::new(&self.0.text()).unwrap()
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn text(&self) -> &[u8] {
        &self.0.text()[self.header().header_len()..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for TcpSegment<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Tcp);
        let _ = TcpHeader::new(ipv4_datagram.text())?;
        Ok(TcpSegment(ipv4_datagram))
    }
}

pub struct TcpSegmentMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> TcpSegmentMut<'a> {
    pub fn new_bytes(text_sz: usize) -> Vec<u8> {
        ipv4::DatagramMut::new_bytes(text_sz + MAX_TCP_HEADER_SIZE)
    }

    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(TcpSegmentMut(ipv4::DatagramMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> TcpHeaderMut<'_> {
        TcpHeaderMut::new(self.0.text())
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn text(&mut self) -> &mut [u8] {
        let header_len = TcpHeader::new(&self.0.text()).unwrap().header_len();
        &mut self.0.text()[header_len..]
    }

    pub fn unmut(self) -> TcpSegment<'a> {
        TcpSegment(self.0.unmut())
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
