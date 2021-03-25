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
    },
    runtime::PacketBuf,
};
use byteorder::{
    ByteOrder,
    NetworkEndian,
};
use std::{
    convert::{
        TryFrom,
        TryInto,
    },
};

pub const UDP_HEADER_SIZE: usize = 8;

#[derive(Debug)]
pub struct UdpHeader {
    pub src_port: Option<ip::Port>,
    pub dst_port: ip::Port,
    // Omit the length and checksum as those are computed when serializing.
    // length: u16,
    // checksum: u16,
}

pub struct UdpDatagram<T: RuntimeBuf> {
    pub ethernet2_hdr: Ethernet2Header,
    pub ipv4_hdr: Ipv4Header,
    pub udp_hdr: UdpHeader,
    pub data: T,

    pub tx_checksum_offload: bool,
}

impl<T: RuntimeBuf> PacketBuf<T> for UdpDatagram<T> {
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size()
            + self.ipv4_hdr.compute_size()
            + self.udp_hdr.compute_size()
    }

    fn body_size(&self) -> usize {
        self.data.len()
    }

    fn write_header(&self, buf: &mut [u8]) {
        let eth_hdr_size = self.ethernet2_hdr.compute_size();
        let ipv4_hdr_size = self.ipv4_hdr.compute_size();
        let udp_hdr_size = self.udp_hdr.compute_size();
        let mut cur_pos = 0;

        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        let ipv4_payload_len = udp_hdr_size + self.data.len();
        self.ipv4_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + ipv4_hdr_size)],
            ipv4_payload_len,
        );
        cur_pos += ipv4_hdr_size;

        self.udp_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + udp_hdr_size)],
            &self.ipv4_hdr,
            &self.data[..],
            self.tx_checksum_offload,
        );
    }

    fn take_body(self) -> Option<T> {
        Some(self.data)
    }
}

impl UdpHeader {
    pub fn compute_size(&self) -> usize {
        UDP_HEADER_SIZE
    }

    pub fn parse<T: RuntimeBuf>(ipv4_header: &Ipv4Header, mut buf: T, rx_checksum_offload: bool) -> Result<(Self, T), Fail> {
        if buf.len() < UDP_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "UDP segment too small",
            });
        }
        let hdr_buf = &buf[..UDP_HEADER_SIZE];

        let src_port = ip::Port::try_from(NetworkEndian::read_u16(&hdr_buf[0..2])).ok();
        let dst_port = ip::Port::try_from(NetworkEndian::read_u16(&hdr_buf[2..4]))?;

        let length = NetworkEndian::read_u16(&hdr_buf[4..6]) as usize;
        if length != buf.len() {
            return Err(Fail::Malformed {
                details: "UDP length mismatch",
            });
        }

        if !rx_checksum_offload {
            let checksum = NetworkEndian::read_u16(&hdr_buf[6..8]);
            if checksum != 0 && checksum != udp_checksum(&ipv4_header, hdr_buf, &buf[UDP_HEADER_SIZE..]) {
                return Err(Fail::Malformed {
                    details: "UDP checksum mismatch",
                });
            }
        }

        let header = Self { src_port, dst_port };
        buf.adjust(UDP_HEADER_SIZE);
        Ok((header, buf))
    }

    pub fn serialize(&self, buf: &mut [u8], ipv4_hdr: &Ipv4Header, data: &[u8], tx_checksum_offload: bool) {
        let fixed_buf: &mut [u8; UDP_HEADER_SIZE] =
            (&mut buf[..UDP_HEADER_SIZE]).try_into().unwrap();

        NetworkEndian::write_u16(
            &mut fixed_buf[0..2],
            self.src_port.map(|p| p.into()).unwrap_or(0),
        );
        NetworkEndian::write_u16(&mut fixed_buf[2..4], self.dst_port.into());
        NetworkEndian::write_u16(&mut fixed_buf[4..6], (UDP_HEADER_SIZE + data.len()) as u16);

        let mut checksum = 0;
        if !tx_checksum_offload {
            checksum = udp_checksum(ipv4_hdr, &fixed_buf[..], data);
        }
        NetworkEndian::write_u16(&mut fixed_buf[6..8], checksum);
    }
}

fn udp_checksum(ipv4_header: &Ipv4Header, header: &[u8], data: &[u8]) -> u16 {
    let mut state = 0xffffu32;

    // First, hash an IPv4 "psuedo header" consisting of...
    // 1) Source address (4 bytes)
    let src_octets = ipv4_header.src_addr.octets();
    state += NetworkEndian::read_u16(&src_octets[0..2]) as u32;
    state += NetworkEndian::read_u16(&src_octets[2..4]) as u32;

    // 2) Destination address (4 bytes)
    let dst_octets = ipv4_header.dst_addr.octets();
    state += NetworkEndian::read_u16(&dst_octets[0..2]) as u32;
    state += NetworkEndian::read_u16(&dst_octets[2..4]) as u32;

    // 3) 1 byte of zeros and UDP protocol number (1 byte)
    state += NetworkEndian::read_u16(&[0, Ipv4Protocol2::Udp as u8]) as u32;

    // 4) UDP segment length (2 bytes)
    state += (header.len() + data.len()) as u32;

    // Then, include the UDP header.
    let fixed_header: &[u8; UDP_HEADER_SIZE] = header.try_into().unwrap();

    // 1) Source port (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[0..2]) as u32;

    // 2) Destination port (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[2..4]) as u32;

    // 3) Length (2 bytes)
    state += NetworkEndian::read_u16(&fixed_header[4..6]) as u32;

    // 4) Checksum (2 bytes, all zeros)
    state += 0;

    // Finally, checksum the data itself.
    let mut chunks_iter = data.chunks_exact(2);
    while let Some(chunk) = chunks_iter.next() {
        state += NetworkEndian::read_u16(chunk) as u32;
    }
    // Since the data may have an odd number of bytes, pad the last byte with zero if necessary.
    if let Some(&b) = chunks_iter.remainder().get(0) {
        state += NetworkEndian::read_u16(&[b, 0]) as u32;
    }

    // See comment for TCP checksum for why doing this once at the end is safe.
    while state > 0xFFFF {
        state -= 0xFFFF;
    }
    !state as u16
}
