// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

pub use header::{UdpHeader, UdpHeaderMut, UDP_HEADER_SIZE};

use crate::{prelude::*, protocols::ipv4};

#[derive(Clone, Copy)]
pub struct UdpDatagramDecoder<'a>(ipv4::Datagram<'a>);

impl<'a> UdpDatagramDecoder<'a> {
    pub fn attach(bytes: &'a [u8]) -> Result<Self> {
        Ok(UdpDatagramDecoder::try_from(ipv4::Datagram::attach(
            bytes,
        )?)?)
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

impl<'a> TryFrom<ipv4::Datagram<'a>> for UdpDatagramDecoder<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        if ipv4_datagram.header().protocol()? != ipv4::Protocol::Udp {
            return Err(Fail::TypeMismatch {
                details: "expected a UDP datagram",
            });
        }

        if ipv4_datagram.text().len() < UDP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "UDP datagram is too small to contain a complete \
                          header",
            });
        }

        Ok(UdpDatagramDecoder(ipv4_datagram))
    }
}

impl<'a> Into<ipv4::Datagram<'a>> for UdpDatagramDecoder<'a> {
    fn into(self) -> ipv4::Datagram<'a> {
        self.0
    }
}

pub struct UdpDatagramEncoder<'a>(ipv4::DatagramMut<'a>);

impl<'a> UdpDatagramEncoder<'a> {
    pub fn new_vec(text_len: usize) -> Vec<u8> {
        let mut bytes = ipv4::Datagram::new_vec(text_len + UDP_HEADER_SIZE);
        let mut datagram = UdpDatagramEncoder::attach(bytes.as_mut());
        datagram.ipv4().header().protocol(ipv4::Protocol::Udp);
        bytes
    }

    pub fn attach(bytes: &'a mut [u8]) -> Self {
        UdpDatagramEncoder(ipv4::DatagramMut::attach(bytes))
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

    #[allow(dead_code)]
    pub fn unmut(&self) -> UdpDatagramDecoder<'_> {
        UdpDatagramDecoder(self.0.unmut())
    }

    pub fn seal(self) -> Result<UdpDatagramDecoder<'a>> {
        trace!("UdpDatagramEncoder::seal()");
        Ok(UdpDatagramDecoder::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for UdpDatagramEncoder<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        UdpDatagramEncoder(ipv4_datagram)
    }
}
