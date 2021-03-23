// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use crate::{
    fail::Fail,
    runtime::RuntimeBuf,
    protocols::{
        ethernet2::frame::{
            Ethernet2Header,
        },
        ip,
        ipv4::datagram::{
            Ipv4Header,
            Ipv4Protocol2,
        },
        tcp::SeqNumber,
    },
    runtime::PacketBuf,
};
use byteorder::{
    ByteOrder,
    NetworkEndian,
    ReadBytesExt,
};
use std::{
    convert::{
        TryFrom,
        TryInto,
    },
    io::Cursor,
    num::Wrapping,
};

pub const MIN_TCP_HEADER_SIZE: usize = 20;
pub const MAX_TCP_HEADER_SIZE: usize = 60;
pub const MAX_TCP_OPTIONS: usize = 5;

pub struct TcpSegment<T: RuntimeBuf> {
    pub ethernet2_hdr: Ethernet2Header,
    pub ipv4_hdr: Ipv4Header,
    pub tcp_hdr: TcpHeader,
    pub data: T,

    pub tx_checksum_offload: bool,
}

impl<T: RuntimeBuf> PacketBuf<T> for TcpSegment<T> {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size()
            + self.ipv4_hdr.compute_size()
            + self.tcp_hdr.compute_size()
    }

    fn body_size(&self) -> usize {
        self.data.len()
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size = self.ethernet2_hdr.compute_size();
        let ipv4_hdr_size = self.ipv4_hdr.compute_size();
        let tcp_hdr_size = self.tcp_hdr.compute_size();
        let mut cur_pos = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        let ipv4_payload_len = tcp_hdr_size + self.data.len();
        self.ipv4_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + ipv4_hdr_size)],
            ipv4_payload_len,
        );
        cur_pos += ipv4_hdr_size;

        self.tcp_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + tcp_hdr_size)],
            &self.ipv4_hdr,
            &self.data[..],
            self.tx_checksum_offload,
        );
    }

    fn take_body(self) -> Option<T> {
        Some(self.data)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SelectiveAcknowlegement {
    pub begin: SeqNumber,
    pub end: SeqNumber,
}

#[derive(Debug, Clone, Copy)]
pub enum TcpOptions2 {
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

impl TcpOptions2 {
    fn compute_size(&self) -> usize {
        use TcpOptions2::*;
        match self {
            NoOperation => 1,
            MaximumSegmentSize(..) => 4,
            WindowScale(..) => 3,
            SelectiveAcknowlegementPermitted => 2,
            SelectiveAcknowlegement { num_sacks, .. } => 2 + 8 * num_sacks,
            Timestamp { .. } => 10,
        }
    }

    fn serialize(&self, buf: &mut [u8]) -> usize {
        use TcpOptions2::*;
        match self {
            NoOperation => {
                buf[0] = 1;
                1
            },
            MaximumSegmentSize(mss) => {
                buf[0] = 2;
                buf[1] = 4;
                NetworkEndian::write_u16(&mut buf[2..4], *mss);
                4
            },
            WindowScale(scale) => {
                buf[0] = 3;
                buf[1] = 3;
                buf[2] = *scale;
                3
            },
            SelectiveAcknowlegementPermitted => {
                buf[0] = 4;
                buf[1] = 2;
                2
            },
            SelectiveAcknowlegement { num_sacks, sacks } => {
                buf[0] = 5;
                buf[1] = 2 + 8 * *num_sacks as u8;
                for i in 0..*num_sacks {
                    NetworkEndian::write_u32(
                        &mut buf[(2 + 8 * i)..(2 + 8 * i + 4)],
                        sacks[i].begin.0,
                    );
                    NetworkEndian::write_u32(
                        &mut buf[(2 + 8 * i + 4)..(2 + 8 * i + 8)],
                        sacks[i].end.0,
                    );
                }
                2 + 8 * num_sacks
            },
            Timestamp {
                sender_timestamp,
                echo_timestamp,
            } => {
                buf[0] = 8;
                buf[1] = 10;
                NetworkEndian::write_u32(&mut buf[2..6], *sender_timestamp);
                NetworkEndian::write_u32(&mut buf[6..10], *echo_timestamp);
                10
            },
        }
    }
}

#[derive(Debug)]
pub struct TcpHeader {
    pub src_port: ip::Port,
    pub dst_port: ip::Port,
    pub seq_num: SeqNumber,
    pub ack_num: SeqNumber,

    // Octet 12: [ data offset in u32s (4 bits) ][ reserved zeros (3 bits) ] [ NS flag ]
    // The data offset is computed on the fly on serialization based on options.
    // data_offset: u8,
    pub ns: bool,

    // Octet 13: [ CWR ] [ ECE ] [ URG ] [ ACK ] [ PSH ] [ RST ] [ SYN ] [ FIN ]
    pub cwr: bool,
    pub ece: bool,
    pub urg: bool,
    pub ack: bool,
    pub psh: bool,
    pub rst: bool,
    pub syn: bool,
    pub fin: bool,

    pub window_size: u16,

    // We omit the checksum since it's checked when parsing and computed when serializing.
    // checksum: u16
    pub urgent_pointer: u16,

    num_options: usize,
    option_list: [TcpOptions2; MAX_TCP_OPTIONS],
}

impl TcpHeader {
    pub fn new(src_port: ip::Port, dst_port: ip::Port) -> Self {
        Self {
            src_port,
            dst_port,
            seq_num: Wrapping(0),
            ack_num: Wrapping(0),

            ns: false,
            cwr: false,
            ece: false,
            urg: false,
            ack: false,
            psh: false,
            rst: false,
            syn: false,
            fin: false,

            window_size: 0,
            urgent_pointer: 0,
            num_options: 0,
            option_list: [TcpOptions2::NoOperation; MAX_TCP_OPTIONS],
        }
    }

    pub fn parse<T: RuntimeBuf>(
        ipv4_header: &Ipv4Header,
        mut buf: T,
        rx_checksum_offload: bool,
    ) -> Result<(Self, T), Fail> {
        if buf.len() < MIN_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "TCP segment too small",
            });
        }
        let data_offset = (buf[12] >> 4) as usize * 4;
        if buf.len() < data_offset {
            return Err(Fail::Malformed {
                details: "TCP segment smaller than data offset",
            });
        }
        if data_offset < MIN_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "TCP data offset too small",
            });
        }
        if data_offset > MAX_TCP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "TCP data offset too large",
            });
        }
        let (hdr_buf, data_buf) = buf[..].split_at(data_offset);

        let src_port = ip::Port::try_from(NetworkEndian::read_u16(&hdr_buf[0..2]))?;
        let dst_port = ip::Port::try_from(NetworkEndian::read_u16(&hdr_buf[2..4]))?;

        let seq_num = Wrapping(NetworkEndian::read_u32(&hdr_buf[4..8]));
        let ack_num = Wrapping(NetworkEndian::read_u32(&hdr_buf[8..12]));

        let ns = (hdr_buf[12] & 1) != 0;

        let cwr = (hdr_buf[13] & (1 << 7)) != 0;
        let ece = (hdr_buf[13] & (1 << 6)) != 0;
        let urg = (hdr_buf[13] & (1 << 5)) != 0;
        let ack = (hdr_buf[13] & (1 << 4)) != 0;
        let psh = (hdr_buf[13] & (1 << 3)) != 0;
        let rst = (hdr_buf[13] & (1 << 2)) != 0;
        let syn = (hdr_buf[13] & (1 << 1)) != 0;
        let fin = (hdr_buf[13] & (1 << 0)) != 0;

        let window_size = NetworkEndian::read_u16(&hdr_buf[14..16]);

        if !rx_checksum_offload {
            let checksum = NetworkEndian::read_u16(&hdr_buf[16..18]);
            if checksum != tcp_checksum(ipv4_header, &hdr_buf[..], &data_buf[..]) {
                return Err(Fail::Malformed {
                    details: "TCP checksum mismatch",
                });
            }
        }

        let urgent_pointer = NetworkEndian::read_u16(&hdr_buf[18..20]);

        let mut num_options = 0;
        let mut option_list = [TcpOptions2::NoOperation; MAX_TCP_OPTIONS];

        if data_offset > MIN_TCP_HEADER_SIZE {
            let mut option_rdr = Cursor::new(&hdr_buf[MIN_TCP_HEADER_SIZE..data_offset]);
            while (option_rdr.position() as usize) < data_offset - MIN_TCP_HEADER_SIZE {
                let option_kind = option_rdr.read_u8()?;
                let option = match option_kind {
                    0 => break,
                    1 => continue,
                    2 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 4 {
                            return Err(Fail::Malformed {
                                details: "MSS size was not 4",
                            });
                        }
                        let mss = option_rdr.read_u16::<NetworkEndian>()?;
                        TcpOptions2::MaximumSegmentSize(mss)
                    },
                    3 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 3 {
                            return Err(Fail::Malformed {
                                details: "Window scale size was not 3",
                            });
                        }
                        let window_scale = option_rdr.read_u8()?;
                        TcpOptions2::WindowScale(window_scale)
                    },
                    4 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 2 {
                            return Err(Fail::Malformed {
                                details: "SACK permitted size was not 2",
                            });
                        }
                        TcpOptions2::SelectiveAcknowlegementPermitted
                    },
                    5 => {
                        let option_length = option_rdr.read_u8()?;
                        let num_sacks = match option_length {
                            10 | 18 | 26 | 34 => (option_length as usize - 2) / 8,
                            _ => {
                                return Err(Fail::Malformed {
                                    details: "Invalid SACK size",
                                })
                            },
                        };
                        let mut sacks = [SelectiveAcknowlegement {
                            begin: Wrapping(0),
                            end: Wrapping(0),
                        }; 4];
                        for i in 0..num_sacks {
                            sacks[i].begin = Wrapping(option_rdr.read_u32::<NetworkEndian>()?);
                            sacks[i].end = Wrapping(option_rdr.read_u32::<NetworkEndian>()?);
                        }
                        TcpOptions2::SelectiveAcknowlegement { num_sacks, sacks }
                    },
                    8 => {
                        let option_length = option_rdr.read_u8()?;
                        if option_length != 10 {
                            return Err(Fail::Malformed {
                                details: "TCP timestamp size was not 10",
                            });
                        }
                        let sender_timestamp = option_rdr.read_u32::<NetworkEndian>()?;
                        let echo_timestamp = option_rdr.read_u32::<NetworkEndian>()?;
                        TcpOptions2::Timestamp {
                            sender_timestamp,
                            echo_timestamp,
                        }
                    },
                    _ => {
                        return Err(Fail::Malformed {
                            details: "Invalid TCP option",
                        })
                    },
                };
                if num_options >= option_list.len() {
                    return Err(Fail::Malformed {
                        details: "Too many TCP options provided",
                    });
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
            ack,
            psh,
            rst,
            syn,
            fin,
            window_size,
            urgent_pointer,

            num_options,
            option_list,
        };
        buf.adjust(data_offset);
        Ok((header, buf))
    }

    pub fn serialize(
        &self,
        buf: &mut [u8],
        ipv4_hdr: &Ipv4Header,
        data: &[u8],
        tx_checksum_offload: bool,
    ) {
        let fixed_buf: &mut [u8; MIN_TCP_HEADER_SIZE] =
            (&mut buf[..MIN_TCP_HEADER_SIZE]).try_into().unwrap();
        NetworkEndian::write_u16(&mut fixed_buf[0..2], self.src_port.into());
        NetworkEndian::write_u16(&mut fixed_buf[2..4], self.dst_port.into());
        NetworkEndian::write_u32(&mut fixed_buf[4..8], self.seq_num.0);
        NetworkEndian::write_u32(&mut fixed_buf[8..12], self.ack_num.0);

        fixed_buf[12] = ((self.compute_size() / 4) as u8) << 4;
        if self.ns {
            fixed_buf[12] |= 1;
        }
        fixed_buf[13] = 0;
        if self.cwr {
            fixed_buf[13] |= 1 << 7;
        }
        if self.ece {
            fixed_buf[13] |= 1 << 6;
        }
        if self.urg {
            fixed_buf[13] |= 1 << 5;
        }
        if self.ack {
            fixed_buf[13] |= 1 << 4;
        }
        if self.psh {
            fixed_buf[13] |= 1 << 3;
        }
        if self.rst {
            fixed_buf[13] |= 1 << 2;
        }
        if self.syn {
            fixed_buf[13] |= 1 << 1;
        }
        if self.fin {
            fixed_buf[13] |= 1 << 0;
        }

        NetworkEndian::write_u16(&mut fixed_buf[14..16], self.window_size);

        // Write the checksum (bytes 16..18) later.

        NetworkEndian::write_u16(&mut fixed_buf[18..20], self.urgent_pointer);

        let mut cur_pos = MIN_TCP_HEADER_SIZE;
        for i in 0..self.num_options {
            let bytes_written = self.option_list[i].serialize(&mut buf[cur_pos..]);
            cur_pos += bytes_written;
        }
        // Write out an "End of options list" if we had options.
        if self.num_options > 0 {
            buf[cur_pos] = 0;
            cur_pos += 1;
        }
        // Zero out the remainder of padding in the header.
        for byte in &mut buf[cur_pos..] {
            *byte = 0;
        }

        // Alright, we've fully filled out the header, time to compute the checksum.
        if !tx_checksum_offload {
            let checksum = tcp_checksum(ipv4_hdr, &buf[..], data);
            NetworkEndian::write_u16(&mut buf[16..18], checksum);
        } else {
            NetworkEndian::write_u16(&mut buf[16..18], 0u16);
        }
    }

    pub fn compute_size(&self) -> usize {
        let mut size = MIN_TCP_HEADER_SIZE;
        for i in 0..self.num_options {
            size += self.option_list[i].compute_size();
        }
        if self.num_options > 0 {
            // Add a byte for the "End of options list" if needed.
            size += 1;
        }

        // Round up to the next multiple of 4 so the TCP data is always 32 bit aligned.
        size.wrapping_add(3) & !0x3
    }

    pub fn iter_options(&self) -> impl Iterator<Item = &TcpOptions2> {
        (0..self.num_options).map(move |i| &self.option_list[i])
    }

    pub fn push_option(&mut self, option: TcpOptions2) {
        self.option_list[self.num_options] = option;
        self.num_options += 1;
    }
}

fn tcp_checksum(ipv4_header: &Ipv4Header, header: &[u8], data: &[u8]) -> u16 {
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

    let fixed_header: &[u8; MIN_TCP_HEADER_SIZE] =
        header[..MIN_TCP_HEADER_SIZE].try_into().unwrap();

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
    state += 0;

    // 8) Urgent pointer (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[18..20]) as u32;

    // Next, the variable length part of the header for TCP options. Since `data_offset` is
    // guaranteed to be aligned to a 32-bit boundary, we don't have to handle remainders.
    if header.len() > MIN_TCP_HEADER_SIZE {
        assert_eq!(header.len() % 2, 0);
        for chunk in header[MIN_TCP_HEADER_SIZE..].chunks_exact(2) {
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
    // loop into a single branchfree code.
    while state > 0xFFFF {
        state -= 0xFFFF;
    }
    !state as u16
}
