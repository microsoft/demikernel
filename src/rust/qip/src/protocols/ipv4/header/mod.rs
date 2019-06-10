mod checksum;

use crate::prelude::*;
use checksum::Hasher;
use num_traits::FromPrimitive;
use std::{convert::TryFrom, net::Ipv4Addr};

const IPV4_VERSION: u32 = 4;
pub const IPV4_HEADER_SIZE: usize = 20;

#[repr(u32)]
#[derive(FromPrimitive, Clone)]
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

// todo: the `bitfield` crate has yet to implement immutable access to fields. see [this github issue](https://github.com/dzamlo/rust-bitfield/issues/23) for details.

bitfield! {
    pub struct Ipv4HeaderMut(MSB0 [u8]);
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
    pub u32, into Ipv4Addr, get_src_addr, _: 103, 96, 4;
    pub u32, into Ipv4Addr, get_dst_addr, _: 159, 128;
}

impl<T> Ipv4HeaderMut<T>
where
    T: AsRef<[u8]> + AsMut<[u8]>,
{
    pub fn validate(bytes: T) -> Result<()> {
        if bytes.as_ref().len() != IPV4_HEADER_SIZE {
            return Err(Fail::Malformed {});
        }

        let should_be_zero = {
            let mut hasher = Hasher::new();
            hasher.write(bytes.as_ref());
            hasher.finish()
        };

        let header: Ipv4HeaderMut<T> = Ipv4HeaderMut(bytes);
        if header.get_version() != IPV4_VERSION {
            return Err(Fail::Unsupported {});
        }

        // from _TCP/IP Illustrated_, Section 5.2.2:
        // > Note that for any nontrivial packet or header, the value
        // > of the Checksum field in the packet can never be FFFF.
        let checksum = header.get_header_checksum();
        if checksum == 0xffff {
            return Err(Fail::Malformed {});
        }

        if checksum != 0 && should_be_zero != 0 {
            return Err(Fail::Malformed {});
        }

        let ihl = header.get_ihl();
        if ihl < 5 {
            return Err(Fail::Malformed {});
        }

        // we don't currently support IPv4 options.
        if ihl > 5 {
            return Err(Fail::Unsupported {});
        }

        let _ = Ipv4Protocol::try_from(header.get_proto())?;
        Ok(())
    }

    /*pub fn recompute_checksum(&mut self) {
        let mut hasher = Hasher::new();
        let bytes = self.0.as_ref();
        hasher.write(&bytes[..10]);
        hasher.write(&bytes[12..]);
        self.set_header_checksum(hasher.finish() as u32);
    }*/
}
