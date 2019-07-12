use super::{TcpOptions, TcpSegment, TcpSegmentMut, MAX_MSS, MIN_MSS};
use crate::protocols::{ethernet2::MacAddress, ip};
use std::{net::Ipv4Addr, num::Wrapping};

#[derive(Default, Clone, Debug)]
pub struct TcpSegmentBuilder {
    dest_ipv4_addr: Option<Ipv4Addr>,
    dest_port: Option<ip::Port>,
    dest_link_addr: Option<MacAddress>,
    src_ipv4_addr: Option<Ipv4Addr>,
    src_port: Option<ip::Port>,
    src_link_addr: Option<MacAddress>,
    seq_num: Option<Wrapping<u32>>,
    ack_num: Option<Wrapping<u32>>,
    syn: bool,
    ack: bool,
    rst: bool,
    mss: Option<usize>,
    payload: Vec<u8>,
}

impl TcpSegmentBuilder {
    pub fn new() -> TcpSegmentBuilder {
        TcpSegmentBuilder::default()
    }

    pub fn build(self) -> Vec<u8> {
        let mut bytes = TcpSegment::new(self.payload.len());
        let mut segment = TcpSegmentMut::attach(bytes.as_mut());
        segment.text()[..self.payload.len()]
            .copy_from_slice(self.payload.as_ref());
        let mut tcp_header = segment.header();
        if let Some(dest_port) = self.dest_port {
            tcp_header.dest_port(dest_port);
        }

        if let Some(src_port) = self.src_port {
            tcp_header.src_port(src_port);
        }

        if let Some(seq_num) = self.seq_num {
            tcp_header.seq_num(seq_num);
        }

        if let Some(ack_num) = self.ack_num {
            tcp_header.ack_num(ack_num);
        }

        tcp_header.syn(self.syn);
        tcp_header.ack(self.ack);
        tcp_header.rst(self.rst);

        let mut options = TcpOptions::new();
        if let Some(mss) = self.mss {
            options.set_mss(mss);
        }

        tcp_header.options(options);

        let ipv4 = segment.ipv4();
        let mut ipv4_header = ipv4.header();
        if let Some(dest_ipv4_addr) = self.dest_ipv4_addr {
            ipv4_header.dest_addr(dest_ipv4_addr);
        }

        if let Some(src_ipv4_addr) = self.src_ipv4_addr {
            ipv4_header.src_addr(src_ipv4_addr);
        }

        let mut frame_header = ipv4.frame().header();
        if let Some(dest_link_addr) = self.dest_link_addr {
            frame_header.dest_addr(dest_link_addr);
        }

        if let Some(src_link_addr) = self.src_link_addr {
            frame_header.src_addr(src_link_addr);
        }

        bytes
    }

    pub fn dest_ipv4_addr(mut self, addr: Ipv4Addr) -> TcpSegmentBuilder {
        self.dest_ipv4_addr = Some(addr);
        self
    }

    pub fn dest_port(mut self, port: ip::Port) -> TcpSegmentBuilder {
        self.dest_port = Some(port);
        self
    }

    pub fn dest_link_addr(mut self, addr: MacAddress) -> TcpSegmentBuilder {
        self.dest_link_addr = Some(addr);
        self
    }

    pub fn src_ipv4_addr(mut self, addr: Ipv4Addr) -> TcpSegmentBuilder {
        self.src_ipv4_addr = Some(addr);
        self
    }

    pub fn src_port(mut self, port: ip::Port) -> TcpSegmentBuilder {
        self.src_port = Some(port);
        self
    }

    pub fn src_link_addr(mut self, addr: MacAddress) -> TcpSegmentBuilder {
        self.src_link_addr = Some(addr);
        self
    }

    pub fn seq_num(mut self, value: Wrapping<u32>) -> TcpSegmentBuilder {
        self.seq_num = Some(value);
        self
    }

    pub fn ack_num(mut self, value: Wrapping<u32>) -> TcpSegmentBuilder {
        self.ack_num = Some(value);
        self
    }

    pub fn syn(mut self) -> TcpSegmentBuilder {
        self.syn = true;
        self
    }

    pub fn ack(mut self) -> TcpSegmentBuilder {
        self.ack = true;
        self
    }

    pub fn rst(mut self) -> TcpSegmentBuilder {
        self.rst = true;
        self
    }

    pub fn mss(mut self, value: usize) -> TcpSegmentBuilder {
        assert!(value >= MIN_MSS);
        assert!(value <= MAX_MSS);
        self.mss = Some(value);
        self
    }

    pub fn payload(mut self, bytes: Vec<u8>) -> TcpSegmentBuilder {
        self.payload = bytes;
        self
    }
}
