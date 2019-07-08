mod header;

use super::checksum::Ipv4Checksum;
use crate::{prelude::*, protocols::ethernet2};
use header::{DEFAULT_IPV4_TTL, IPV4_IHL_NO_OPTIONS, IPV4_VERSION};
use std::{convert::TryFrom, io::Write};

pub use header::{Ipv4Header, Ipv4HeaderMut, Ipv4Protocol, IPV4_HEADER_SIZE};

pub struct Ipv4Datagram<'a>(ethernet2::Frame<'a>);

impl<'a> Ipv4Datagram<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        Ok(Ipv4Datagram::try_from(ethernet2::Frame::from_bytes(
            bytes,
        )?)?)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0.as_bytes()
    }

    pub fn header(&self) -> Ipv4Header<'_> {
        Ipv4Header::new(&self.0.text()[..IPV4_HEADER_SIZE])
    }

    #[allow(dead_code)]
    pub fn frame(&self) -> &ethernet2::Frame<'a> {
        &self.0
    }

    pub fn text(&self) -> &[u8] {
        &self.0.text()[IPV4_HEADER_SIZE..]
    }
}

impl<'a> TryFrom<ethernet2::Frame<'a>> for Ipv4Datagram<'a> {
    type Error = Fail;

    fn try_from(frame: ethernet2::Frame<'a>) -> Result<Self> {
        trace!("Ipv4Datagram::try_from(...)");
        assert_eq!(frame.header().ether_type()?, ethernet2::EtherType::Ipv4);
        if frame.text().len() <= IPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "IPv4 datagram is too small to contain a complete \
                          header",
            });
        }

        let datagram = Ipv4Datagram(frame);
        let text_len = datagram.text().len();
        let header = datagram.header();
        if header.version() != IPV4_VERSION {
            return Err(Fail::Unsupported {
                details: "unsupported IPv4 version",
            });
        }

        if header.total_len() != text_len + IPV4_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "IPv4 TOTALLEN mismatch",
            });
        }

        let ihl = header.ihl();
        if ihl < IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Malformed {
                details: "IPv4 IHL is too small",
            });
        }

        // we don't currently support IPv4 options.
        if ihl > IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Unsupported {
                details: "IPv4 options are not supported",
            });
        }

        // we don't currently support fragmented packets.
        if header.frag_offset() != 0 {
            return Err(Fail::Unsupported {
                details: "IPv4 fragmentation is not supported",
            });
        }

        // from _TCP/IP Illustrated_, Section 5.2.2:
        // > Note that for any nontrivial packet or header, the value
        // > of the Checksum field in the packet can never be FFFF.
        let checksum = header.checksum();
        if checksum == 0xffff {
            return Err(Fail::Malformed {
                details: "invalid IPv4 checksum",
            });
        }

        let should_be_zero = {
            let mut checksum = Ipv4Checksum::new();
            checksum.write_all(header.as_bytes()).unwrap();
            checksum.finish()
        };

        if checksum != 0 && should_be_zero != 0 {
            return Err(Fail::Malformed {
                details: "IPv4 checksum mismatch",
            });
        }

        let _ = header.protocol()?;
        Ok(datagram)
    }
}

pub struct Ipv4DatagramMut<'a>(ethernet2::FrameMut<'a>);

impl<'a> Ipv4DatagramMut<'a> {
    pub fn new_bytes(text_sz: usize) -> Vec<u8> {
        ethernet2::FrameMut::new_bytes(text_sz + IPV4_HEADER_SIZE)
    }

    pub fn from_bytes(bytes: &'a mut [u8]) -> Result<Self> {
        Ok(Ipv4DatagramMut(ethernet2::FrameMut::from_bytes(bytes)?))
    }

    pub fn header(&mut self) -> Ipv4HeaderMut<'_> {
        Ipv4HeaderMut::new(&mut self.0.text()[..IPV4_HEADER_SIZE])
    }

    pub fn frame(&mut self) -> &mut ethernet2::FrameMut<'a> {
        &mut self.0
    }

    pub fn text(&mut self) -> &mut [u8] {
        &mut self.0.text()[IPV4_HEADER_SIZE..]
    }

    pub fn unmut(self) -> Ipv4Datagram<'a> {
        Ipv4Datagram(self.0.unmut())
    }

    pub fn seal(mut self) -> Result<Ipv4Datagram<'a>> {
        trace!("Ipv4DatagramMut::seal()");
        let text_len = self.text().len();
        let total_len = IPV4_HEADER_SIZE + text_len;

        {
            let mut ipv4_header = self.header();
            ipv4_header.version(IPV4_VERSION);
            ipv4_header.ihl(IPV4_IHL_NO_OPTIONS);
            ipv4_header.ttl(DEFAULT_IPV4_TTL);
            ipv4_header.total_len(u16::try_from(total_len)?);

            let mut checksum = Ipv4Checksum::new();
            checksum.write_all(&ipv4_header.as_bytes()[..10]).unwrap();
            checksum.write_all(&ipv4_header.as_bytes()[12..]).unwrap();
            ipv4_header.checksum(checksum.finish());
        }

        let mut frame_header = self.frame().header();
        frame_header.ether_type(ethernet2::EtherType::Ipv4);
        Ok(self.unmut())
    }
}

impl<'a> From<ethernet2::FrameMut<'a>> for Ipv4DatagramMut<'a> {
    fn from(frame: ethernet2::FrameMut<'a>) -> Self {
        Ipv4DatagramMut(frame)
    }
}
