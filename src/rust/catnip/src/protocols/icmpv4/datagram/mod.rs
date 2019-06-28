mod header;

pub use header::{
    Icmpv4Header, Icmpv4HeaderMut, Icmpv4Type, ICMPV4_HEADER_SIZE,
};

use crate::{prelude::*, protocols::ipv4};
use std::{cmp::min, convert::TryFrom, io::Write};

const MAX_ICMPV4_DATAGRAM_SIZE: usize = 576;

pub struct Icmpv4Datagram<'a>(ipv4::Datagram<'a>);

impl<'a> Icmpv4Datagram<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        Ok(Icmpv4Datagram::try_from(ipv4::Datagram::from_bytes(
            bytes,
        )?)?)
    }

    pub fn header(&self) -> Icmpv4Header<'_> {
        Icmpv4Header::new(&self.0.payload()[..ICMPV4_HEADER_SIZE])
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload()[ICMPV4_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for Icmpv4Datagram<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        assert_eq!(ipv4_datagram.header().protocol()?, ipv4::Protocol::Icmpv4);
        if ipv4_datagram.payload().len() < ICMPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "ICMPv4 datagram isn't large enough to contain a \
                          complete header",
            });
        }

        let icmpv4 = Icmpv4Datagram(ipv4_datagram);
        let mut checksum = ipv4::Checksum::new();
        let payload = icmpv4.ipv4().payload();
        checksum.write_all(&payload[..2]).unwrap();
        checksum.write_all(&payload[4..]).unwrap();
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
    pub fn new_bytes(payload_sz: usize) -> Vec<u8> {
        let mut bytes =
            ipv4::DatagramMut::new_bytes(ICMPV4_HEADER_SIZE + payload_sz);
        bytes.resize(min(bytes.len(), MAX_ICMPV4_DATAGRAM_SIZE), 0);
        bytes
    }

    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(Icmpv4DatagramMut(ipv4::DatagramMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> Icmpv4HeaderMut<'_> {
        Icmpv4HeaderMut::new(&mut self.0.payload()[..ICMPV4_HEADER_SIZE])
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn payload(&mut self) -> &mut [u8] {
        &mut self.0.payload()[ICMPV4_HEADER_SIZE..]
    }

    pub fn unmut(self) -> Icmpv4Datagram<'a> {
        Icmpv4Datagram(self.0.unmut())
    }

    pub fn seal(mut self) -> Result<Icmpv4Datagram<'a>> {
        trace!("Icmp4DatagramMut::seal()");
        self.ipv4().header().protocol(ipv4::Protocol::Icmpv4);
        self.header().checksum(0);
        let mut checksum = ipv4::Checksum::new();
        checksum.write_all(self.0.payload()).unwrap();
        self.header().checksum(checksum.finish());
        Ok(Icmpv4Datagram::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for Icmpv4DatagramMut<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        Icmpv4DatagramMut(ipv4_datagram)
    }
}
