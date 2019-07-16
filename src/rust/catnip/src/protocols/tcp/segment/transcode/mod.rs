mod header;

#[cfg(test)]
mod tests;

pub use header::{
    TcpHeaderDecoder, TcpHeaderEncoder, TcpSegmentOptions, DEFAULT_MSS,
    MAX_TCP_HEADER_SIZE, MIN_TCP_HEADER_SIZE,
};

use crate::{prelude::*, protocols::ipv4};
use byteorder::{NetworkEndian, WriteBytesExt};
use std::{convert::TryFrom, io::Write};

#[derive(Debug)]
enum ChecksumOp {
    Generate,
    Validate,
}

pub struct TcpSegmentDecoder<'a>(ipv4::Datagram<'a>);

impl<'a> TcpSegmentDecoder<'a> {
    pub fn attach(bytes: &'a [u8]) -> Result<Self> {
        Ok(TcpSegmentDecoder::try_from(ipv4::Datagram::attach(bytes)?)?)
    }

    pub fn header(&self) -> TcpHeaderDecoder<'_> {
        // the header contents were validated when `try_from()` was called.
        TcpHeaderDecoder::attach(&self.0.text()).unwrap()
    }

    pub fn ipv4(&self) -> &ipv4::Datagram<'a> {
        &self.0
    }

    pub fn text(&self) -> &[u8] {
        &self.0.text()[self.header().header_len()..]
    }

    fn checksum(&self, op: ChecksumOp) -> Result<u16> {
        let mut checksum = ipv4::Checksum::new();
        let ipv4_header = self.0.header();
        checksum
            .write_u32::<NetworkEndian>(ipv4_header.src_addr().into())
            .unwrap();
        checksum
            .write_u32::<NetworkEndian>(ipv4_header.dest_addr().into())
            .unwrap();
        checksum.write_u8(0u8).unwrap();
        checksum.write_u8(ipv4_header.protocol()?.into()).unwrap();
        let mut tcp_len = self.text().len();
        let tcp_header = self.header();
        let header_len = tcp_header.header_len();
        tcp_len += header_len;
        let tcp_len = u16::try_from(tcp_len)?;
        checksum.write_u16::<NetworkEndian>(tcp_len).unwrap();
        let src_port = match tcp_header.src_port() {
            Some(p) => p.into(),
            None => 0,
        };

        checksum.write_u16::<NetworkEndian>(src_port).unwrap();

        let dest_port = match tcp_header.dest_port() {
            Some(p) => p.into(),
            None => 0,
        };

        checksum.write_u16::<NetworkEndian>(dest_port).unwrap();
        checksum
            .write_u32::<NetworkEndian>(tcp_header.seq_num().0)
            .unwrap();
        checksum
            .write_u32::<NetworkEndian>(tcp_header.ack_num().0)
            .unwrap();
        // write TCP header length & flags
        checksum.write_all(&tcp_header.as_bytes()[12..14]).unwrap();
        checksum
            .write_u16::<NetworkEndian>(tcp_header.window_sz())
            .unwrap();

        match op {
            ChecksumOp::Generate => {
                checksum.write_u16::<NetworkEndian>(0u16).unwrap();
            }
            ChecksumOp::Validate => {
                checksum
                    .write_u16::<NetworkEndian>(tcp_header.checksum())
                    .unwrap();
            }
        }

        checksum
            .write_u16::<NetworkEndian>(tcp_header.urg_ptr())
            .unwrap();
        checksum
            .write_all(&tcp_header.as_bytes()[MIN_TCP_HEADER_SIZE..])
            .unwrap();
        checksum.write_all(self.text()).unwrap();

        match op {
            ChecksumOp::Validate => {
                if checksum.finish() == 0 {
                    Ok(0)
                } else {
                    Err(Fail::Malformed {
                        details: "TCP checksum mismatch",
                    })
                }
            }
            ChecksumOp::Generate => Ok(checksum.finish()),
        }
    }
}

impl<'a> TryFrom<ipv4::Datagram<'a>> for TcpSegmentDecoder<'a> {
    type Error = Fail;

    fn try_from(ipv4_datagram: ipv4::Datagram<'a>) -> Result<Self> {
        if ipv4_datagram.header().protocol()? != ipv4::Protocol::Tcp {
            return Err(Fail::TypeMismatch {
                details: "expected a TCP segment",
            });
        }

        let _ = TcpHeaderDecoder::attach(ipv4_datagram.text())?;
        let segment = TcpSegmentDecoder(ipv4_datagram);
        let _ = segment.checksum(ChecksumOp::Validate)?;
        Ok(segment)
    }
}

pub struct TcpSegmentEncoder<'a>(ipv4::DatagramMut<'a>);

impl<'a> TcpSegmentEncoder<'a> {
    pub fn attach(bytes: &'a mut [u8]) -> Self {
        TcpSegmentEncoder(ipv4::DatagramMut::attach(bytes))
    }

    pub fn header(&mut self) -> TcpHeaderEncoder<'_> {
        TcpHeaderEncoder::attach(self.0.text())
    }

    pub fn ipv4(&mut self) -> &mut ipv4::DatagramMut<'a> {
        &mut self.0
    }

    pub fn text(&mut self) -> &mut [u8] {
        let header_len = TcpHeaderDecoder::attach(&self.0.text())
            .unwrap()
            .header_len();
        &mut self.0.text()[header_len..]
    }

    pub fn unmut(&self) -> TcpSegmentDecoder<'_> {
        TcpSegmentDecoder(self.0.unmut())
    }

    pub fn seal(mut self) -> Result<TcpSegmentDecoder<'a>> {
        trace!("TcpSegmentEncoder::seal()");
        let checksum = self.unmut().checksum(ChecksumOp::Generate).unwrap();
        let mut tcp_header = self.header();
        tcp_header.checksum(checksum);
        Ok(TcpSegmentDecoder::try_from(self.0.seal()?)?)
    }
}

impl<'a> From<ipv4::DatagramMut<'a>> for TcpSegmentEncoder<'a> {
    fn from(ipv4_datagram: ipv4::DatagramMut<'a>) -> Self {
        TcpSegmentEncoder(ipv4_datagram)
    }
}
