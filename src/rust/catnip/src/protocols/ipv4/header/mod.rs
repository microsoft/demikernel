#[cfg(test)]
mod tests;

pub mod checksum;

use crate::prelude::*;
use checksum::Hasher;
use num_traits::FromPrimitive;
use std::{
    convert::TryFrom,
    io::{Read, Write},
    net::Ipv4Addr,
};

const IPV4_HEADER_SIZE: usize = 20;
// todo: need citation
const DEFAULT_IPV4_TTL: u32 = 64;
const IPV4_IHL_NO_OPTIONS: u32 = 5;
const IPV4_VERSION: u32 = 4;

bitfield! {
    pub struct BitsMut(MSB0 [u8]);
    impl Debug;
    u32;
    pub get_version, set_version: 3, 0;
    pub get_ihl, set_ihl: 7, 4;
    pub get_dscp, set_dscp: 13, 8;
    pub get_ecn, set_ecn: 15, 14;
    pub get_total_len, set_total_len: 31, 16;
    pub get_id, set_id: 47, 31;
    pub get_df, set_df: 49;
    pub get_mf, set_mf: 50;
    pub get_frag_offset, set_frag_offset: 63, 51;
    pub get_ttl, set_ttl: 71, 64;
    pub get_proto, set_proto: 79, 72;
    pub get_header_checksum, set_header_checksum: 95, 79;
    pub get_src_addr, set_src_addr: 103, 96;
    pub get_dest_addr, set_dest_addr: 159, 128;
}

#[repr(u32)]
#[derive(FromPrimitive, Clone, PartialEq, Eq, Debug)]
pub enum Ipv4Protocol {
    Udp = 0x11,
}

impl TryFrom<u32> for Ipv4Protocol {
    type Error = Fail;

    fn try_from(n: u32) -> Result<Self> {
        match FromPrimitive::from_u32(n) {
            Some(n) => Ok(n),
            None => Err(Fail::Unsupported {}),
        }
    }
}

impl Into<u32> for Ipv4Protocol {
    fn into(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Ipv4Header {
    pub protocol: Ipv4Protocol,
    pub src_addr: Ipv4Addr,
    pub dest_addr: Ipv4Addr,
}

impl Ipv4Header {
    pub fn read(reader: &mut Read, payload_len: usize) -> Result<Self> {
        trace!("*");
        // todo: the `bitfield` crate has yet to implement immutable access to fields. see [this github issue](https://github.com/dzamlo/rust-bitfield/issues/23) for details.
        let mut bytes = [0; IPV4_HEADER_SIZE];
        reader.read_exact(&mut bytes)?;

        let should_be_zero = {
            let mut hasher = Hasher::new();
            hasher.write(&bytes);
            hasher.finish()
        };

        let bits = BitsMut(&mut bytes);

        debug!("a {}", bits.get_version());
        if bits.get_version() != IPV4_VERSION {
            return Err(Fail::Unsupported {});
        }

        if bits.get_total_len() as usize != payload_len + Ipv4Header::size() {
            return Err(Fail::Malformed {});
        }

        let ihl = bits.get_ihl();
        if ihl < IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Malformed {});
        }

        debug!("b");
        // we don't currently support IPv4 options.
        if ihl > IPV4_IHL_NO_OPTIONS {
            return Err(Fail::Unsupported {});
        }

        debug!("c");
        // we don't currently support fragmented packets.
        if bits.get_frag_offset() != 0 {
            return Err(Fail::Unsupported {});
        }

        // from _TCP/IP Illustrated_, Section 5.2.2:
        // > Note that for any nontrivial packet or header, the value
        // > of the Checksum field in the packet can never be FFFF.
        let checksum = bits.get_header_checksum();
        if checksum == 0xffff {
            return Err(Fail::Malformed {});
        }

        if checksum != 0 && should_be_zero != 0 {
            return Err(Fail::Malformed {});
        }

        debug!("d");
        let protocol = Ipv4Protocol::try_from(bits.get_proto())?;
        Ok(Ipv4Header {
            protocol,
            src_addr: Ipv4Addr::from(bits.get_src_addr()),
            dest_addr: Ipv4Addr::from(bits.get_dest_addr()),
        })
    }

    pub fn write(&self, writer: &mut Write, payload_len: usize) -> Result<()> {
        let mut bytes = [0; IPV4_HEADER_SIZE];

        {
            let mut bits = BitsMut(&mut bytes);
            bits.set_version(IPV4_VERSION);
            bits.set_ihl(IPV4_IHL_NO_OPTIONS);
            bits.set_ttl(DEFAULT_IPV4_TTL);
            bits.set_total_len(u32::from(u16::try_from(
                payload_len + Ipv4Header::size(),
            )?));
            bits.set_proto(self.protocol.clone().into());
            bits.set_src_addr(self.src_addr.into());
            bits.set_dest_addr(self.dest_addr.into());
        }

        let mut hasher = Hasher::new();
        hasher.write(&bytes[..10]);
        hasher.write(&bytes[12..]);

        {
            let mut bits = BitsMut(&mut bytes);
            bits.set_header_checksum(u32::from(hasher.finish()));
        }

        writer.write_all(&bytes)?;
        Ok(())
    }

    pub fn size() -> usize {
        IPV4_HEADER_SIZE
    }
}
