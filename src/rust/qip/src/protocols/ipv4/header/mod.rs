mod checksum;

use crate::prelude::*;
use checksum::Hasher;
use std::net::Ipv4Addr;

const IPV4_VERSION: u32 = 4;
pub const IPV4_HEADER_SIZE: usize = 20;

bitfield! {
    pub struct Ipv4Header(MSB0 [u8]);
    impl Debug;
    u32;
    get_version, _: 3, 0;
    get_ihl, _: 7, 4;
    get_dscp, _: 13, 8;
    get_ecn, _: 15, 14;
    get_total_len, _: 31, 16;
    get_id, _: 47, 31;
    get_df, _: 49;
    get_mf, _: 50;
    get_frag_offset, _: 63, 51;
    get_ttl, _: 71, 64;
    get_proto, _: 79, 72;
    get_header_checksum, set_header_checksum: 95, 79;
    u32, into Ipv4Addr, get_src_addr, _: 103, 96, 4;
    u32, into Ipv4Addr, get_dst_addr, _: 159, 128;
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> Ipv4Header<T> {
    pub fn new(mut bytes: T) -> Result<Ipv4Header<T>> {
        if bytes.as_mut().len() != IPV4_HEADER_SIZE {
            return Err(Fail::Malformed {});
        }

        let should_be_zero = {
            let mut hasher = Hasher::new();
            hasher.write(bytes.as_ref());
            hasher.finish()
        };

        let header: Ipv4Header<T> = Ipv4Header(bytes);
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

        Ok(header)
    }

    pub fn recompute_checksum(&mut self) {
        let mut hasher = Hasher::new();
        let bytes = self.0.as_ref();
        hasher.write(&bytes[..10]);
        hasher.write(&bytes[12..]);
        self.set_header_checksum(hasher.finish() as u32);
    }
}
