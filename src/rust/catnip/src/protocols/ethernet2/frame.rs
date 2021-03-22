use crate::{
    fail::Fail,
    protocols::ethernet2::MacAddress,
    runtime::RuntimeBuf,
};
use byteorder::{
    ByteOrder,
    NetworkEndian,
};
use num_traits::FromPrimitive;
use std::convert::{
    TryFrom,
    TryInto,
};

pub const MIN_PAYLOAD_SIZE: usize = 46;
pub const ETHERNET2_HEADER_SIZE: usize = 14;

#[repr(u16)]
#[derive(FromPrimitive, Copy, Clone, PartialEq, Eq, Debug)]
pub enum EtherType2 {
    Arp = 0x806,
    Ipv4 = 0x800,
}

impl TryFrom<u16> for EtherType2 {
    type Error = Fail;

    fn try_from(n: u16) -> Result<Self, Fail> {
        match FromPrimitive::from_u16(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {
                details: "Unsupported ETHERTYPE",
            }),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ethernet2Header {
    // Bytes 0..6
    pub dst_addr: MacAddress,
    // Bytes 6..12
    pub src_addr: MacAddress,
    // Bytes 12..14
    pub ether_type: EtherType2,
}

impl Ethernet2Header {
    pub fn compute_size(&self) -> usize {
        ETHERNET2_HEADER_SIZE
    }

    pub fn parse<T: RuntimeBuf>(mut buf: T) -> Result<(Self, T), Fail> {
        if buf.len() < ETHERNET2_HEADER_SIZE {
            return Err(Fail::Malformed {
                details: "Frame too small",
            });
        }
        let hdr_buf = &buf[..ETHERNET2_HEADER_SIZE];
        let dst_addr = MacAddress::from_bytes(&hdr_buf[0..6]);
        let src_addr = MacAddress::from_bytes(&hdr_buf[6..12]);
        let ether_type = EtherType2::try_from(NetworkEndian::read_u16(&hdr_buf[12..14]))?;
        let hdr = Self {
            dst_addr,
            src_addr,
            ether_type,
        };

        buf.adjust(ETHERNET2_HEADER_SIZE);
        Ok((hdr, buf))
    }

    pub fn serialize(&self, buf: &mut [u8]) {
        let buf: &mut [u8; ETHERNET2_HEADER_SIZE] = buf.try_into().unwrap();
        buf[0..6].copy_from_slice(&self.dst_addr.octets());
        buf[6..12].copy_from_slice(&self.src_addr.octets());
        NetworkEndian::write_u16(&mut buf[12..14], self.ether_type as u16);
    }
}
