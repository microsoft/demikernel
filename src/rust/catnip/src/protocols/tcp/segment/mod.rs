mod transcode;

use super::connection::TcpConnection;
use crate::{
    prelude::*,
    protocols::{ethernet2::MacAddress, ip, ipv4},
};
use std::{convert::TryFrom, net::Ipv4Addr, num::Wrapping};

pub use transcode::{
    TcpSegmentDecoder, TcpSegmentEncoder, TcpSegmentOptions, DEFAULT_MSS,
    MAX_MSS, MIN_MSS,
};

#[derive(Default, Clone, Debug)]
pub struct TcpSegment {
    pub dest_ipv4_addr: Option<Ipv4Addr>,
    pub dest_port: Option<ip::Port>,
    pub dest_link_addr: Option<MacAddress>,
    pub src_ipv4_addr: Option<Ipv4Addr>,
    pub src_port: Option<ip::Port>,
    pub src_link_addr: Option<MacAddress>,
    pub seq_num: Wrapping<u32>,
    pub ack_num: Wrapping<u32>,
    pub syn: bool,
    pub ack: bool,
    pub rst: bool,
    pub mss: Option<usize>,
    pub payload: Vec<u8>,
}

impl TcpSegment {
    pub fn dest_ipv4_addr(mut self, addr: Ipv4Addr) -> TcpSegment {
        self.dest_ipv4_addr = Some(addr);
        self
    }

    pub fn dest_port(mut self, port: ip::Port) -> TcpSegment {
        self.dest_port = Some(port);
        self
    }

    pub fn dest_link_addr(mut self, addr: MacAddress) -> TcpSegment {
        self.dest_link_addr = Some(addr);
        self
    }

    pub fn src_ipv4_addr(mut self, addr: Ipv4Addr) -> TcpSegment {
        self.src_ipv4_addr = Some(addr);
        self
    }

    pub fn src_port(mut self, port: ip::Port) -> TcpSegment {
        self.src_port = Some(port);
        self
    }

    pub fn src_link_addr(mut self, addr: MacAddress) -> TcpSegment {
        self.src_link_addr = Some(addr);
        self
    }

    pub fn seq_num(mut self, value: Wrapping<u32>) -> TcpSegment {
        self.seq_num = value;
        self
    }

    pub fn ack_num(mut self, value: Wrapping<u32>) -> TcpSegment {
        self.ack_num = value;
        self
    }

    pub fn syn(mut self) -> TcpSegment {
        self.syn = true;
        self
    }

    pub fn ack(mut self) -> TcpSegment {
        self.ack = true;
        self
    }

    pub fn rst(mut self) -> TcpSegment {
        self.rst = true;
        self
    }

    pub fn mss(mut self, value: usize) -> TcpSegment {
        self.mss = Some(value);
        self
    }

    pub fn payload(mut self, bytes: Vec<u8>) -> TcpSegment {
        self.payload = bytes;
        self
    }

    pub fn connection(self, cxn: &TcpConnection) -> TcpSegment {
        self.src_ipv4_addr(cxn.id.local.address())
            .src_port(cxn.id.local.port())
            .dest_ipv4_addr(cxn.id.remote.address())
            .dest_port(cxn.id.remote.port())
            .seq_num(cxn.seq_num)
    }

    pub fn decode(bytes: &[u8]) -> Result<TcpSegment> {
        TcpSegment::try_from(TcpSegmentDecoder::attach(bytes)?)
    }

    pub fn encode(self) -> Vec<u8> {
        let mut options = TcpSegmentOptions::new();
        if let Some(mss) = self.mss {
            options.set_mss(mss);
        }

        let mut bytes = ipv4::Datagram::new_vec(
            self.payload.len() + options.header_length(),
        );
        let mut encoder = TcpSegmentEncoder::attach(bytes.as_mut());

        encoder.text()[..self.payload.len()]
            .copy_from_slice(self.payload.as_ref());

        let mut tcp_header = encoder.header();
        tcp_header.dest_port(self.dest_port.unwrap());
        tcp_header.src_port(self.src_port.unwrap());
        tcp_header.seq_num(self.seq_num);
        tcp_header.ack_num(self.ack_num);
        tcp_header.syn(self.syn);
        tcp_header.ack(self.ack);
        tcp_header.rst(self.rst);
        tcp_header.options(options);

        let ipv4 = encoder.ipv4();
        let mut ipv4_header = ipv4.header();
        ipv4_header.protocol(ipv4::Protocol::Tcp);
        ipv4_header.dest_addr(self.dest_ipv4_addr.unwrap());
        ipv4_header.src_addr(self.src_ipv4_addr.unwrap());

        let mut frame_header = ipv4.frame().header();
        frame_header.dest_addr(self.dest_link_addr.unwrap());
        frame_header.src_addr(self.src_link_addr.unwrap());

        let _ = encoder.seal().expect("failed to seal TCP segment");
        bytes
    }
}

impl<'a> TryFrom<TcpSegmentDecoder<'a>> for TcpSegment {
    type Error = Fail;

    fn try_from(decoder: TcpSegmentDecoder<'a>) -> Result<TcpSegment> {
        let tcp_header = decoder.header();
        let dest_port = tcp_header.dest_port();
        let src_port = tcp_header.src_port();
        let seq_num = tcp_header.seq_num();
        let ack_num = tcp_header.ack_num();
        let syn = tcp_header.syn();
        let ack = tcp_header.ack();
        let rst = tcp_header.rst();
        let options = tcp_header.options();
        let mss = options.get_mss();

        let ipv4_header = decoder.ipv4().header();
        let dest_ipv4_addr = ipv4_header.dest_addr();
        let src_ipv4_addr = ipv4_header.src_addr();

        let frame_header = decoder.ipv4().frame().header();
        let src_link_addr = frame_header.src_addr();
        let dest_link_addr = frame_header.dest_addr();
        let payload = decoder.text().to_vec();

        Ok(TcpSegment {
            dest_ipv4_addr: Some(dest_ipv4_addr),
            dest_port,
            dest_link_addr: Some(dest_link_addr),
            src_ipv4_addr: Some(src_ipv4_addr),
            src_port,
            src_link_addr: Some(src_link_addr),
            seq_num,
            ack_num,
            syn,
            ack,
            rst,
            mss,
            payload,
        })
    }
}
