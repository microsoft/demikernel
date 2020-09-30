// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod transcode;

use bytes::{Bytes, BytesMut};
use crate::{
    protocols::{ethernet::MacAddress, ip, ipv4},
};
use crate::fail::Fail;
use std::{convert::TryFrom, net::Ipv4Addr, num::Wrapping};

pub use transcode::{
    TcpSegmentDecoder, TcpSegmentEncoder, TcpSegmentOptions, DEFAULT_MSS,
    MAX_MSS, MIN_MSS,
};

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct TcpSegment {
    pub dest_ipv4_addr: Option<Ipv4Addr>,
    pub dest_port: Option<ip::Port>,
    pub dest_link_addr: Option<MacAddress>,
    pub src_ipv4_addr: Option<Ipv4Addr>,
    pub src_port: Option<ip::Port>,
    pub src_link_addr: Option<MacAddress>,
    pub seq_num: Wrapping<u32>,
    pub ack_num: Wrapping<u32>,
    pub window_size: usize,
    pub syn: bool,
    pub ack: bool,
    pub rst: bool,
    pub fin: bool,
    pub mss: Option<usize>,
    pub window_scale: Option<u8>,
    pub payload: Bytes,
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

    pub fn ack(mut self, ack_num: Wrapping<u32>) -> TcpSegment {
        self.ack = true;
        self.ack_num = ack_num;
        self
    }

    pub fn window_size(mut self, window_size: usize) -> TcpSegment {
        self.window_size = window_size;
        self
    }

    pub fn syn(mut self) -> TcpSegment {
        self.syn = true;
        self
    }

    pub fn fin(mut self) -> TcpSegment {
        self.fin = true;
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

    pub fn payload(mut self, bytes: Bytes) -> TcpSegment {
        self.payload = bytes;
        self
    }

    pub fn decode(bytes: &[u8]) -> Result<TcpSegment, Fail> {
        TcpSegment::try_from(TcpSegmentDecoder::attach(bytes)?)
    }

    pub fn encode(self) -> Vec<u8> {
        trace!("TcpSegment::encode()");
        let mut options = TcpSegmentOptions::new();
        if let Some(mss) = self.mss {
            options.set_mss(mss);
        }

        let mut bytes = ipv4::Datagram::new_vec(
            self.payload.len() + options.header_length(),
        );
        let mut encoder = TcpSegmentEncoder::attach(bytes.as_mut());

        let mut tcp_header = encoder.header();
        tcp_header.dest_port(self.dest_port.unwrap());
        tcp_header.src_port(self.src_port.unwrap());
        tcp_header.seq_num(self.seq_num);
        tcp_header.ack_num(self.ack_num);
        tcp_header.window_size(u16::try_from(self.window_size).unwrap());
        tcp_header.syn(self.syn);
        tcp_header.ack(self.ack);
        tcp_header.rst(self.rst);
        tcp_header.options(options);

        // setting the TCP options shifts where `encoder.text()` begins, so we
        // need to ensure that we copy the payload after the options are set.
        encoder.text()[..self.payload.len()]
            .copy_from_slice(self.payload.as_ref());

        let ipv4 = encoder.ipv4();
        let mut ipv4_header = ipv4.header();
        ipv4_header.protocol(ipv4::Protocol::Tcp);
        ipv4_header.dest_addr(self.dest_ipv4_addr.unwrap());

        // the source IPv4 address is set within `TcpPeerState::cast()` so this
        // field is not likely to be filled in in advance.
        if let Some(src_ipv4_addr) = self.src_ipv4_addr {
            ipv4_header.src_addr(src_ipv4_addr);
        }

        let mut frame_header = ipv4.frame().header();
        // link layer addresses are filled in after the ARP request is
        // complete, so these fields won't be filled in in advance.
        if let Some(dest_link_addr) = self.dest_link_addr {
            frame_header.dest_addr(dest_link_addr);
        }

        if let Some(src_link_addr) = self.src_link_addr {
            frame_header.src_addr(src_link_addr);
        }

        encoder.write_checksum();

        bytes
    }
}

impl<'a> TryFrom<TcpSegmentDecoder<'a>> for TcpSegment {
    type Error = Fail;

    fn try_from(decoder: TcpSegmentDecoder<'a>) -> Result<Self, Fail> {
        let tcp_header = decoder.header();
        let dest_port = tcp_header.dest_port();
        let src_port = tcp_header.src_port();
        let seq_num = tcp_header.seq_num();
        let ack_num = tcp_header.ack_num();
        let syn = tcp_header.syn();
        let ack = tcp_header.ack();
        let rst = tcp_header.rst();
        let fin = tcp_header.fin();
        let options = tcp_header.options();
        let mss = options.get_mss();
        let window_size = usize::from(tcp_header.window_size());
        let window_scale = options.get_window_scale();

        let ipv4_header = decoder.ipv4().header();
        let dest_ipv4_addr = ipv4_header.dest_addr();
        let src_ipv4_addr = ipv4_header.src_addr();

        let frame_header = decoder.ipv4().frame().header();
        let src_link_addr = frame_header.src_addr();
        let dest_link_addr = frame_header.dest_addr();
        let payload = BytesMut::from(decoder.text()).freeze();

        Ok(TcpSegment {
            dest_ipv4_addr: Some(dest_ipv4_addr),
            dest_port,
            dest_link_addr: Some(dest_link_addr),
            src_ipv4_addr: Some(src_ipv4_addr),
            src_port,
            src_link_addr: Some(src_link_addr),
            seq_num,
            ack_num,
            window_size,
            syn,
            ack,
            rst,
            mss,
            fin,
            payload,
            window_scale,
        })
    }
}
