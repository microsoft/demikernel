// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod transcode;

use crate::fail::Fail;
pub use transcode::{
    UdpDatagramDecoder, UdpDatagramEncoder, UdpHeader, UdpHeaderMut,
    UDP_HEADER_SIZE,
};
use crate::{
    protocols::{ethernet2::MacAddress, ip, ipv4},
};
use std::{convert::TryFrom, net::Ipv4Addr};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct UdpDatagram {
    pub dest_ipv4_addr: Option<Ipv4Addr>,
    pub dest_link_addr: Option<MacAddress>,
    pub dest_port: Option<ip::Port>,
    pub src_ipv4_addr: Option<Ipv4Addr>,
    pub src_port: Option<ip::Port>,
    pub src_link_addr: Option<MacAddress>,
    pub payload: Vec<u8>,
}

impl UdpDatagram {
    pub fn dest_ipv4_addr(mut self, addr: Ipv4Addr) -> UdpDatagram {
        self.dest_ipv4_addr = Some(addr);
        self
    }

    pub fn dest_port(mut self, port: ip::Port) -> UdpDatagram {
        self.dest_port = Some(port);
        self
    }

    pub fn dest_link_addr(mut self, addr: MacAddress) -> UdpDatagram {
        self.dest_link_addr = Some(addr);
        self
    }

    pub fn src_ipv4_addr(mut self, addr: Ipv4Addr) -> UdpDatagram {
        self.src_ipv4_addr = Some(addr);
        self
    }

    pub fn src_port(mut self, port: ip::Port) -> UdpDatagram {
        self.src_port = Some(port);
        self
    }

    pub fn src_link_addr(mut self, addr: MacAddress) -> UdpDatagram {
        self.src_link_addr = Some(addr);
        self
    }

    pub fn payload(mut self, bytes: Vec<u8>) -> UdpDatagram {
        self.payload = bytes;
        self
    }

    pub fn decode(bytes: &[u8]) -> Result<UdpDatagram, Fail> {
        UdpDatagram::try_from(UdpDatagramDecoder::attach(bytes)?)
    }

    pub fn encode(self) -> Vec<u8> {
        let mut bytes =
            ipv4::Datagram::new_vec(self.payload.len() + UDP_HEADER_SIZE);
        let mut encoder = UdpDatagramEncoder::attach(bytes.as_mut());

        encoder.text()[..self.payload.len()]
            .copy_from_slice(self.payload.as_ref());

        let mut udp_header = encoder.header();
        udp_header.dest_port(self.dest_port.unwrap());
        udp_header.src_port(self.src_port.unwrap());

        let ipv4 = encoder.ipv4();
        let mut ipv4_header = ipv4.header();
        ipv4_header.protocol(ipv4::Protocol::Udp);
        ipv4_header.dest_addr(self.dest_ipv4_addr.unwrap());
        ipv4_header.src_addr(self.src_ipv4_addr.unwrap());

        let mut frame_header = ipv4.frame().header();
        frame_header.dest_addr(self.dest_link_addr.unwrap());
        frame_header.src_addr(self.src_link_addr.unwrap());

        let _ = encoder.seal().expect("failed to seal TCP segment");
        bytes
    }
}

impl<'a> TryFrom<UdpDatagramDecoder<'a>> for UdpDatagram {
    type Error = Fail;

    fn try_from(decoder: UdpDatagramDecoder<'a>) -> Result<Self, Fail> {
        let udp_header = decoder.header();
        let dest_port = udp_header.dest_port();
        let src_port = udp_header.src_port();

        let ipv4_header = decoder.ipv4().header();
        let dest_ipv4_addr = ipv4_header.dest_addr();
        let src_ipv4_addr = ipv4_header.src_addr();

        let frame_header = decoder.ipv4().frame().header();
        let src_link_addr = frame_header.src_addr();
        let dest_link_addr = frame_header.dest_addr();
        let payload = decoder.text().to_vec();

        Ok(UdpDatagram {
            dest_ipv4_addr: Some(dest_ipv4_addr),
            dest_port,
            dest_link_addr: Some(dest_link_addr),
            src_ipv4_addr: Some(src_ipv4_addr),
            src_port,
            src_link_addr: Some(src_link_addr),
            payload,
        })
    }
}
