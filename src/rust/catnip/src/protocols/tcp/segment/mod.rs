// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod transcode;

use std::io::Cursor;
use std::convert::TryInto;
use crate::{
    fail::Fail,
    protocols::{
        ethernet::MacAddress,
        ip,
        ipv4,
    },
};
use crate::protocols::ipv4::datagram::{Ipv4Protocol2, Ipv4Header2};
use bytes::{
    Bytes,
    BytesMut,
};
use byteorder::{ByteOrder, NetworkEndian, ReadBytesExt};
use std::{
    convert::TryFrom,
    net::Ipv4Addr,
    num::Wrapping,
};

pub use transcode::{
    TcpSegmentDecoder,
    TcpSegmentEncoder,
    TcpSegmentOptions,
    DEFAULT_MSS,
    MAX_MSS,
    MIN_MSS,
};
use crate::protocols::tcp::SeqNumber;

const MIN_TCP_HEADER2_SIZE: usize = 20;
const MAX_TCP_HEADER2_SIZE: usize = 60;
const MAX_TCP_OPTIONS: usize = 5;

#[derive(Clone, Copy)]
struct SelectiveAcknowlegement {
    begin: SeqNumber,
    end: SeqNumber,
}

#[derive(Clone, Copy)]
enum TcpOptions2 {
    NoOperation,
    MaximumSegmentSize(u16),
    WindowScale(u8),
    SelectiveAcknowlegementPermitted,
    SelectiveAcknowlegement {
        num_sacks: usize,
        sacks: [SelectiveAcknowlegement; 4],
    },
    Timestamp {
        sender_timestamp: u32,
        echo_timestamp: u32,
    },
}

pub struct TcpHeader2 {
    src_port: ip::Port,
    dst_port: ip::Port,
    seq_num: SeqNumber,
    ack_num: SeqNumber,

    // Octet 12: [ data offset in u32s (4 bits) ][ reserved zeros (3 bits) ] [ NS flag ]
    // The data offset is computed on the fly on serialization based on options.
    // data_offset: u8,
    ns: bool,

    // Octet 13: [ CWR ] [ ECE ] [ URG ] [ ACK ] [ PSH ] [ RST ] [ SYN ] [ FIN ]
    cwr: bool,
    ece: bool,
    urg: bool,
    psh: bool,
    rst: bool,
    syn: bool,
    fin: bool,

    window_size: u16,

    // We omit the checksum since it's checked when parsing and computed when serializing.
    // checksum: u16

    urgent_pointer: u16,

    num_options: usize,
    option_list: [TcpOptions2; MAX_TCP_OPTIONS],
}

impl TcpHeader2 {
    pub fn parse(ipv4_header: &Ipv4Header2, mut buf: Bytes) -> Result<(Self, Bytes), Fail> {
        if buf.len() < MIN_TCP_HEADER2_SIZE {
            return Err(Fail::Malformed { details: "TCP segment too small" });
        }
        let data_offset = (buf[12] >> 4) as usize * 4;
        if buf.len() < data_offset {
            return Err(Fail::Malformed { details: "TCP segment smaller than data offset" });
        }
        if data_offset < MIN_TCP_HEADER2_SIZE {
            return Err(Fail::Malformed { details: "TCP data offset too small" });
        }
        if data_offset > MAX_TCP_HEADER2_SIZE {
            return Err(Fail::Malformed { details: "TCP data offset too large" });
        }
        let data_buf = buf.split_off(data_offset);

        let src_port = ip::Port::try_from(NetworkEndian::read_u16(&buf[0..2]))?;
        let dst_port = ip::Port::try_from(NetworkEndian::read_u16(&buf[3..4]))?;

        let seq_num = Wrapping(NetworkEndian::read_u32(&buf[4..8]));
        let ack_num = Wrapping(NetworkEndian::read_u32(&buf[8..12]));

        let ns = (buf[12] & 1) != 0;

        let cwr = (buf[13] & (1 << 7)) != 0;
        let ece = (buf[13] & (1 << 6)) != 0;
        let urg = (buf[13] & (1 << 5)) != 0;
        let ack = (buf[13] & (1 << 4)) != 0;
        let psh = (buf[13] & (1 << 3)) != 0;
        let rst = (buf[13] & (1 << 2)) != 0;
        let syn = (buf[13] & (1 << 1)) != 0;
        let fin = (buf[13] & (1 << 0)) != 0;

        let window_size = NetworkEndian::read_u16(&buf[14..16]);

        let checksum = NetworkEndian::read_u16(&buf[16..18]);
        if checksum != tcp_checksum(ipv4_header, &buf[..], &data_buf[..]) {
            return Err(Fail::Malformed { details: "TCP checksum mismatch" });
        }

        let urgent_pointer = NetworkEndian::read_u16(&buf[18..20]);

        let mut num_options = 0;
        let mut option_list = [TcpOptions2::NoOperation; MAX_TCP_OPTIONS];

        if data_offset > MIN_TCP_HEADER2_SIZE {
            let mut option_rdr = Cursor::new(&buf[MIN_TCP_HEADER2_SIZE..data_offset]);
            loop {
                let option_kind = option_rdr.read_u8()?;
                let option = match option_kind {
                    0 => break,
                    1 => continue,
                    2 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 4 {
                            return Err(Fail::Malformed { details: "MSS size was not 4" });
                        }
                        let mss = option_rdr.read_u16::<NetworkEndian>()?;
                        TcpOptions2::MaximumSegmentSize(mss)
                    },
                    3 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 3 {
                            return Err(Fail::Malformed { details: "Window scale size was not 3" });
                        }
                        let window_scale = option_rdr.read_u8()?;
                        TcpOptions2::WindowScale(window_scale)
                    },
                    4 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 2 {
                            return Err(Fail::Malformed { details: "SACK permitted size was not 2" });
                        }
                        TcpOptions2::SelectiveAcknowlegementPermitted
                    },
                    5 => {
                        let option_length = option_rdr.read_u8()?;
                        let num_sacks = match option_length {
                            10 | 18 | 26 | 34 => (option_length as usize - 2) / 8,
                            _ => return Err(Fail::Malformed { details: "Invalid SACK size" }),
                        };
                        let mut sacks = [SelectiveAcknowlegement { begin: Wrapping(0), end: Wrapping(0)}; 4];
                        for i in 0..num_sacks {
                            sacks[i].begin = Wrapping(option_rdr.read_u32::<NetworkEndian>()?);
                            sacks[i].end = Wrapping(option_rdr.read_u32::<NetworkEndian>()?);
                        }
                        TcpOptions2::SelectiveAcknowlegement { num_sacks, sacks }
                    },
                    8 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 10 {
                            return Err(Fail::Malformed { details: "TCP timestamp size was not 10" });
                        }
                        let sender_timestamp = option_rdr.read_u32::<NetworkEndian>()?;
                        let echo_timestamp = option_rdr.read_u32::<NetworkEndian>()?;
                        TcpOptions2::Timestamp { sender_timestamp, echo_timestamp }
                    },
                    _ => return Err(Fail::Malformed { details: "Invalid TCP option" }),
                };
                if num_options >= option_list.len() {
                    return Err(Fail::Malformed { details: "Too many TCP options provided" });
                }
                option_list[num_options] = option;
                num_options += 1;
            }
        }

        let header = Self {
            src_port,
            dst_port,
            seq_num,
            ack_num,
            ns,
            cwr,
            ece,
            urg,
            psh,
            rst,
            syn,
            fin,
            window_size,
            urgent_pointer,

            num_options,
            option_list,
        };
        Ok((header, data_buf))

    }
}

fn tcp_checksum(ipv4_header: &Ipv4Header2, header: &[u8], data: &[u8]) -> u16 {
    let mut state = 0xffffu32;

    // First, fold in a "pseudo-IP" header of...
    // 1) Source address (4 bytes)
    let src_octets = ipv4_header.src_addr.octets();
    state += NetworkEndian::read_u16(&src_octets[0..2]) as u32;
    state += NetworkEndian::read_u16(&src_octets[2..4]) as u32;

    // 2) Destination address (4 bytes)
    let dst_octets = ipv4_header.dst_addr.octets();
    state += NetworkEndian::read_u16(&dst_octets[0..2]) as u32;
    state += NetworkEndian::read_u16(&dst_octets[2..4]) as u32;

    // 3) 1 byte of zeros and TCP protocol number (1 byte)
    state += NetworkEndian::read_u16(&[0, Ipv4Protocol2::Tcp as u8]) as u32;

    // 4) TCP segment length (2 bytes)
    state += (header.len() + data.len()) as u32;

    let fixed_header: &[u8; MIN_TCP_HEADER2_SIZE] = header[..MIN_TCP_HEADER2_SIZE].try_into().unwrap();

    // Continue to the TCP header. First, for the fixed length parts, we have...
    // 1) Source port (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[0..2]) as u32;

    // 2) Destination port (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[2..4]) as u32;

    // 3) Sequence number (4 bytes)
    state += NetworkEndian::read_u16(&fixed_header[4..6]) as u32;
    state += NetworkEndian::read_u16(&fixed_header[6..8]) as u32;

    // 4) Acknowledgement number (4 bytes)
    state += NetworkEndian::read_u16(&fixed_header[8..10]) as u32;
    state += NetworkEndian::read_u16(&fixed_header[10..12]) as u32;

    // 5) Data offset (4 bits), reserved (4 bits), and flags (1 byte)
    state += NetworkEndian::read_u16(&fixed_header[12..14]) as u32;

    // 6) Window (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[14..16]) as u32;

    // 7) Checksum (all zeros, 2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[16..18]) as u32;

    // 8) Urgent pointer (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[18..20]) as u32;

    // Next, the variable length part of the header for TCP options. Since `data_offset` is
    // guaranteed to be aligned to a 32-bit boundary, we don't have to handle remainders.
    if header.len() > MIN_TCP_HEADER2_SIZE {
        assert_eq!(header.len() % 2, 0);
        for chunk in header[MIN_TCP_HEADER2_SIZE..].chunks_exact(2) {
            state += NetworkEndian::read_u16(chunk) as u32;
        }
    }

    // Finally, checksum the data itself.
    let mut chunks_iter = data.chunks_exact(2);
    while let Some(chunk) = chunks_iter.next() {
        state += NetworkEndian::read_u16(chunk) as u32;
    }
    // Since the data may have an odd number of bytes, pad the last byte with zero if necessary.
    if let Some(&b) = chunks_iter.remainder().get(0) {
        state += NetworkEndian::read_u16(&[b, 0]) as u32;
    }

    // NB: We don't need to subtract out 0xFFFF as we accumulate the sum. Since we use a u32 for
    // intermediate state, we would need 2^16 additions to overflow. This is well beyond the reach
    // of the largest jumbo frames. The upshot is that the compiler can then optimize this final
    // loop into a branchfree algorithm.
    while state > 0xFFFF {
        state -= 0xFFFF;
    }
    !state as u16
}


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

        let mut bytes = ipv4::Datagram::new_vec(self.payload.len() + options.header_length());
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
        encoder.text()[..self.payload.len()].copy_from_slice(self.payload.as_ref());

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
