// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    expect_ok,
    inetstack::protocols::{
        ethernet2::Ethernet2Header,
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::SeqNumber,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::PacketBuf,
    },
};
use ::libc::EBADMSG;
use ::std::{
    io::{
        Cursor,
        Read,
    },
    slice::ChunksExact,
};

pub const MIN_TCP_HEADER_SIZE: usize = 20;
pub const MAX_TCP_HEADER_SIZE: usize = 60;
pub const MAX_TCP_OPTIONS: usize = 5;

pub struct TcpSegment {
    pub ethernet2_hdr: Ethernet2Header,
    pub ipv4_hdr: Ipv4Header,
    pub tcp_hdr: TcpHeader,
    pub data: Option<DemiBuffer>,
    pub tx_checksum_offload: bool,
}

impl PacketBuf for TcpSegment {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size() + self.ipv4_hdr.compute_size() + self.tcp_hdr.compute_size()
    }

    fn body_size(&self) -> usize {
        match &self.data {
            Some(buf) => buf.len(),
            None => 0,
        }
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size: usize = self.ethernet2_hdr.compute_size();
        let ipv4_hdr_size: usize = self.ipv4_hdr.compute_size();
        let tcp_hdr_size: usize = self.tcp_hdr.compute_size();
        let mut cur_pos: usize = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        let ipv4_payload_len = tcp_hdr_size + self.body_size();
        self.ipv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + ipv4_hdr_size)], ipv4_payload_len);
        cur_pos += ipv4_hdr_size;

        let payload: &[u8] = match &self.data {
            Some(buf) => &buf[..],
            None => &[],
        };
        self.tcp_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + tcp_hdr_size)],
            &self.ipv4_hdr,
            payload,
            self.tx_checksum_offload,
        );
    }

    fn take_body(&self) -> Option<DemiBuffer> {
        match &self.data {
            Some(body) => Some(body.clone()),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectiveAcknowlegement {
    pub begin: SeqNumber,
    pub end: SeqNumber,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TcpOptions2 {
    EndOfOptionsList,
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
            EndOfOptionsList => 0,
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
            EndOfOptionsList => 0,
            NoOperation => {
                buf[0] = 1;
                1
            },
            MaximumSegmentSize(mss) => {
                buf[0] = 2;
                buf[1] = 4;
                buf[2..4].copy_from_slice(&mss.to_be_bytes());
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
                    buf[(2 + 8 * i)..(2 + 8 * i + 4)].copy_from_slice(&u32::from(sacks[i].begin).to_be_bytes());
                    buf[(2 + 8 * i + 4)..(2 + 8 * i + 8)].copy_from_slice(&u32::from(sacks[i].end).to_be_bytes());
                }
                2 + 8 * num_sacks
            },
            Timestamp {
                sender_timestamp,
                echo_timestamp,
            } => {
                buf[0] = 8;
                buf[1] = 10;
                buf[2..6].copy_from_slice(&sender_timestamp.to_be_bytes());
                buf[6..10].copy_from_slice(&echo_timestamp.to_be_bytes());
                10
            },
        }
    }
}

#[derive(Debug)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
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

    pub num_options: usize,
    pub option_list: [TcpOptions2; MAX_TCP_OPTIONS],
}

impl TcpHeader {
    pub fn new(src_port: u16, dst_port: u16) -> Self {
        Self {
            src_port,
            dst_port,
            seq_num: SeqNumber::from(0),
            ack_num: SeqNumber::from(0),

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

    pub fn parse(
        ipv4_header: &Ipv4Header,
        mut buf: DemiBuffer,
        rx_checksum_offload: bool,
    ) -> Result<(Self, DemiBuffer), Fail> {
        if buf.len() < MIN_TCP_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "TCP segment too small"));
        }
        let data_offset: usize = (buf[12] >> 4) as usize * 4;
        if buf.len() < data_offset {
            return Err(Fail::new(EBADMSG, "TCP segment smaller than data offset"));
        }
        if data_offset < MIN_TCP_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "TCP data offset too small"));
        }
        if data_offset > MAX_TCP_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "TCP data offset too large"));
        }
        let (hdr_buf, data_buf): (&[u8], &[u8]) = buf[..].split_at(data_offset);

        let src_port: u16 = u16::from_be_bytes([hdr_buf[0], hdr_buf[1]]);
        let dst_port: u16 = u16::from_be_bytes([hdr_buf[2], hdr_buf[3]]);

        let seq_num: SeqNumber = SeqNumber::from(u32::from_be_bytes([hdr_buf[4], hdr_buf[5], hdr_buf[6], hdr_buf[7]]));
        let ack_num: SeqNumber =
            SeqNumber::from(u32::from_be_bytes([hdr_buf[8], hdr_buf[9], hdr_buf[10], hdr_buf[11]]));

        let ns: bool = (hdr_buf[12] & 1) != 0;

        let cwr: bool = (hdr_buf[13] & (1 << 7)) != 0;
        let ece: bool = (hdr_buf[13] & (1 << 6)) != 0;
        let urg: bool = (hdr_buf[13] & (1 << 5)) != 0;
        let ack: bool = (hdr_buf[13] & (1 << 4)) != 0;
        let psh: bool = (hdr_buf[13] & (1 << 3)) != 0;
        let rst: bool = (hdr_buf[13] & (1 << 2)) != 0;
        let syn: bool = (hdr_buf[13] & (1 << 1)) != 0;
        let fin: bool = (hdr_buf[13] & (1 << 0)) != 0;

        let window_size: u16 = u16::from_be_bytes([hdr_buf[14], hdr_buf[15]]);

        if !rx_checksum_offload {
            let checksum: u16 = u16::from_be_bytes([hdr_buf[16], hdr_buf[17]]);
            if checksum != tcp_checksum(ipv4_header, hdr_buf, data_buf) {
                return Err(Fail::new(EBADMSG, "TCP checksum mismatch"));
            }
        }

        let urgent_pointer: u16 = u16::from_be_bytes([hdr_buf[18], hdr_buf[19]]);

        let mut num_options: usize = 0;
        let mut option_list: [TcpOptions2; MAX_TCP_OPTIONS] = [TcpOptions2::NoOperation; MAX_TCP_OPTIONS];

        if data_offset > MIN_TCP_HEADER_SIZE {
            let mut option_rdr: Cursor<&[u8]> = Cursor::new(&hdr_buf[MIN_TCP_HEADER_SIZE..data_offset]);
            while (option_rdr.position() as usize) < data_offset - MIN_TCP_HEADER_SIZE {
                let mut temp: [u8; 1] = [0; 1];
                option_rdr.read_exact(&mut temp)?;
                let option_kind: u8 = temp[0];
                let option: TcpOptions2 = match option_kind {
                    0 => break,
                    1 => continue,
                    2 => {
                        let mut temp: [u8; 1] = [0; 1];
                        option_rdr.read_exact(&mut temp)?;
                        let option_length: u8 = temp[0];
                        if option_length != 4 {
                            return Err(Fail::new(EBADMSG, "MSS size was not 4"));
                        }
                        let mut temp: [u8; 2] = [0; 2];
                        option_rdr.read_exact(&mut temp)?;
                        let mss: u16 = u16::from_be_bytes([temp[0], temp[1]]);
                        TcpOptions2::MaximumSegmentSize(mss)
                    },
                    3 => {
                        let mut temp: [u8; 1] = [0; 1];
                        option_rdr.read_exact(&mut temp)?;
                        let option_length: u8 = temp[0];
                        if option_length != 3 {
                            return Err(Fail::new(EBADMSG, "window scale size was not 3"));
                        }
                        option_rdr.read_exact(&mut temp)?;
                        let window_scale: u8 = temp[0];
                        TcpOptions2::WindowScale(window_scale)
                    },
                    4 => {
                        let mut temp: [u8; 1] = [0; 1];
                        option_rdr.read_exact(&mut temp)?;
                        let option_length: u8 = temp[0];
                        if option_length != 2 {
                            return Err(Fail::new(EBADMSG, "SACK permitted size was not 2"));
                        }
                        TcpOptions2::SelectiveAcknowlegementPermitted
                    },
                    5 => {
                        let mut temp: [u8; 1] = [0; 1];
                        option_rdr.read_exact(&mut temp)?;
                        let option_length: u8 = temp[0];
                        let num_sacks: usize = match option_length {
                            10 | 18 | 26 | 34 => (option_length as usize - 2) / 8,
                            _ => return Err(Fail::new(EBADMSG, "invalid SACK size")),
                        };
                        let mut sacks: [SelectiveAcknowlegement; 4] = [SelectiveAcknowlegement {
                            begin: SeqNumber::from(0),
                            end: SeqNumber::from(0),
                        }; 4];
                        for s in sacks.iter_mut().take(num_sacks) {
                            let mut temp: [u8; 4] = [0; 4];
                            option_rdr.read_exact(&mut temp)?;
                            s.begin = SeqNumber::from(u32::from_be_bytes([temp[0], temp[1], temp[2], temp[3]]));
                            option_rdr.read_exact(&mut temp)?;
                            s.end = SeqNumber::from(u32::from_be_bytes([temp[0], temp[1], temp[2], temp[3]]));
                        }
                        TcpOptions2::SelectiveAcknowlegement { num_sacks, sacks }
                    },
                    8 => {
                        let mut temp: [u8; 1] = [0; 1];
                        option_rdr.read_exact(&mut temp)?;
                        let option_length: u8 = temp[0];
                        if option_length != 10 {
                            return Err(Fail::new(EBADMSG, "TCP timestamp size was not 10"));
                        }
                        let mut temp: [u8; 4] = [0; 4];
                        option_rdr.read_exact(&mut temp)?;
                        let sender_timestamp: u32 = u32::from_be_bytes([temp[0], temp[1], temp[2], temp[3]]);
                        option_rdr.read_exact(&mut temp)?;
                        let echo_timestamp: u32 = u32::from_be_bytes([temp[0], temp[1], temp[2], temp[3]]);
                        TcpOptions2::Timestamp {
                            sender_timestamp,
                            echo_timestamp,
                        }
                    },
                    _ => return Err(Fail::new(EBADMSG, "invalid TCP option")),
                };
                if num_options >= option_list.len() {
                    return Err(Fail::new(EBADMSG, "too many TCP options provided"));
                }
                option_list[num_options] = option;
                num_options += 1;
            }
        }

        let header: TcpHeader = Self {
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
        expect_ok!(
            buf.adjust(data_offset),
            "buf should contain at least 'data_offset' bytes"
        );
        Ok((header, buf))
    }

    pub fn serialize(&self, buf: &mut [u8], ipv4_hdr: &Ipv4Header, data: &[u8], tx_checksum_offload: bool) {
        let fixed_buf: &mut [u8; MIN_TCP_HEADER_SIZE] = (&mut buf[..MIN_TCP_HEADER_SIZE]).try_into().unwrap();
        fixed_buf[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        fixed_buf[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        fixed_buf[4..8].copy_from_slice(&u32::from(self.seq_num).to_be_bytes());
        fixed_buf[8..12].copy_from_slice(&u32::from(self.ack_num).to_be_bytes());
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

        fixed_buf[14..16].copy_from_slice(&self.window_size.to_be_bytes());

        // Write the checksum (bytes 16..18) later.

        fixed_buf[18..20].copy_from_slice(&self.urgent_pointer.to_be_bytes());

        let mut cur_pos: usize = MIN_TCP_HEADER_SIZE;
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
            let checksum: u16 = tcp_checksum(ipv4_hdr, &buf[..], data);
            buf[16..18].copy_from_slice(&checksum.to_be_bytes());
        } else {
            buf[16] = 0;
            buf[17] = 0;
        }
    }

    // TODO: Review the use of usize here (and everywhere in inetstack, really).
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
        // TODO: Review why wrapping_add is used here.
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
    let mut state: u32 = 0xffff;

    // First, fold in a "pseudo-IP" header of...
    // 1) Source address (4 bytes)
    let src_octets: [u8; 4] = ipv4_header.get_src_addr().octets();
    state += u16::from_be_bytes([src_octets[0], src_octets[1]]) as u32;
    state += u16::from_be_bytes([src_octets[2], src_octets[3]]) as u32;

    // 2) Destination address (4 bytes)
    let dst_octets: [u8; 4] = ipv4_header.get_dest_addr().octets();
    state += u16::from_be_bytes([dst_octets[0], dst_octets[1]]) as u32;
    state += u16::from_be_bytes([dst_octets[2], dst_octets[3]]) as u32;

    // 3) 1 byte of zeros and TCP protocol number (1 byte)
    state += u16::from_be_bytes([0, IpProtocol::TCP as u8]) as u32;

    // 4) TCP segment length (2 bytes)
    state += (header.len() + data.len()) as u32;

    let fixed_header: &[u8; MIN_TCP_HEADER_SIZE] = header[..MIN_TCP_HEADER_SIZE].try_into().unwrap();

    // Continue to the TCP header. First, for the fixed length parts, we have...
    // 1) Source port (2 bytes)
    state += u16::from_be_bytes([fixed_header[0], fixed_header[1]]) as u32;

    // 2) Destination port (2 bytes)
    state += u16::from_be_bytes([fixed_header[2], fixed_header[3]]) as u32;

    // 3) Sequence number (4 bytes)
    state += u16::from_be_bytes([fixed_header[4], fixed_header[5]]) as u32;
    state += u16::from_be_bytes([fixed_header[6], fixed_header[7]]) as u32;

    // 4) Acknowledgement number (4 bytes)
    state += u16::from_be_bytes([fixed_header[8], fixed_header[9]]) as u32;
    state += u16::from_be_bytes([fixed_header[10], fixed_header[11]]) as u32;

    // 5) Data offset (4 bits), reserved (4 bits), and flags (1 byte)
    state += u16::from_be_bytes([fixed_header[12], fixed_header[13]]) as u32;

    // 6) Window (2 bytes)
    state += u16::from_be_bytes([fixed_header[14], fixed_header[15]]) as u32;

    // 7) Checksum (all zeros, 2 bytes)
    state += 0;

    // 8) Urgent pointer (2 bytes)
    state += u16::from_be_bytes([fixed_header[18], fixed_header[19]]) as u32;

    // Next, the variable length part of the header for TCP options. Since `data_offset` is
    // guaranteed to be aligned to a 32-bit boundary, we don't have to handle remainders.
    if header.len() > MIN_TCP_HEADER_SIZE {
        assert_eq!(header.len() % 2, 0);
        for chunk in header[MIN_TCP_HEADER_SIZE..].chunks_exact(2) {
            state += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
        }
    }

    // Finally, checksum the data itself.
    let mut chunks_iter: ChunksExact<u8> = data.chunks_exact(2);
    while let Some(chunk) = chunks_iter.next() {
        state += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    // Since the data may have an odd number of bytes, pad the last byte with zero if necessary.
    if let Some(&b) = chunks_iter.remainder().get(0) {
        state += u16::from_be_bytes([b, 0]) as u32;
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
