// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;

#[cfg(test)]
mod tests;

pub use header::{
    Icmpv4Header, Icmpv4HeaderMut, Icmpv4Type, ICMPV4_HEADER_SIZE,
};

use crate::{prelude::*, protocols::ipv4};
use byteorder::{NetworkEndian, WriteBytesExt};
use std::{convert::TryFrom, io::Write};

const MAX_ICMPV4_DATAGRAM_SIZE: usize = 576;

#[derive(Clone, Copy)]
pub struct Icmpv4Datagram<'a>(ipv4::Datagram<'a>);

impl<'a> Icmpv4Datagram<'a> {
    pub fn new_vec(text_len: usize) -> Vec<u8> {
        let mut bytes = ipv4::Datagram::new_vec(ICMPV4_HEADER_SIZE + text_len);
        assert!(bytes.len() <= MAX_ICMPV4_DATAGRAM_SIZE);
        let mut datagram = Icmpv4DatagramMut::attach(bytes.as_mut());
        datagram.ipv4().header().protocol(ipv4::Protocol::Icmpv4);
        bytes
    }

    #[allow(dead_code)]
    pub fn attach(bytes: &'a [u8]) -> Result<Self> {
        Ok(Icmpv4Datagram::try_from(ipv4::Datagram::attach(bytes)?)?)
    }

    pub fn header(&self) -> Icmpv4Header<'_> {
        Icmpv4Header::new(&self.0.text()[..ICMPV4_HEADER_SIZE])
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn text(&self) -> &[u8] {
        &self.0.text()[ICMPV4_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for Icmpv4Datagram<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        if ipv4_datagram.header().protocol()? != ipv4::Protocol::Icmpv4 {
            return Err(Fail::TypeMismatch {
                details: "expected a ICMPv4 datagram",
            });
        }

        if ipv4_datagram.text().len() < ICMPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "ICMPv4 datagram isn't large enough to contain a \
                          complete header",
            });
        }

        let icmpv4 = Icmpv4Datagram(ipv4_datagram);
        let mut checksum = ipv4::Checksum::new();
        let text = icmpv4.ipv4().text();
        checksum.write_all(&text[..2]).unwrap();
        checksum.write_all(&text[4..]).unwrap();
        if checksum.finish() != icmpv4.header().checksum() {
            return Err(Fail::Malformed {
                details: "ICMPv4 checksum mismatch",
            });
        }

        Ok(icmpv4)
    }
}

pub struct Icmpv4DatagramMut<'a>(ipv4::DatagramMut<'a>);

impl<'a> Icmpv4DatagramMut<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> Self {
        Icmpv4DatagramMut(ipv4::DatagramMut::attach(bytes))
    }

    pub fn header(&mut self) -> Icmpv4HeaderMut<'_> {
        Icmpv4HeaderMut::new(&mut self.0.text()[..ICMPV4_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn text(&mut self) -> &mut [u8] {
        &mut self.0.text()[ICMPV4_HEADER_SIZE..]
    }

    pub fn unmut(&self) -> Icmpv4Datagram<'_> {
        Icmpv4Datagram(self.0.unmut())
    }

    pub fn seal(mut self) -> Result<Icmpv4Datagram<'a>> {
        trace!("Icmp4DatagramMut::seal()");
        let ipv4_text = self.0.text();
        let mut checksum = ipv4::Checksum::new();
        checksum.write_all(&ipv4_text[..2]).unwrap();
        checksum.write_u16::<NetworkEndian>(0u16).unwrap();
        checksum.write_all(&ipv4_text[4..]).unwrap();
        let mut icmp_header = self.header();
        icmp_header.checksum(checksum.finish());
        Ok(Icmpv4Datagram::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for Icmpv4DatagramMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        Icmpv4DatagramMut(ipv4_datagram)
    }
}
