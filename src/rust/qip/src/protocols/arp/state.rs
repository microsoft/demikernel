use super::cache::ArpCache;
use super::pdu::{ArpOp, ArpPdu};
use crate::prelude::*;
use crate::protocols::ethernet2;
use std::convert::TryFrom;
use std::mem::swap;
use std::time::Instant;

pub struct ArpState {
    options: Options,
    cache: ArpCache,
}

impl ArpState {
    pub fn from_options(options: &Options, now: Instant) -> ArpState {
        ArpState {
            options: options.clone(),
            cache: ArpCache::from_options(&options.arp.cache, now),
        }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.cache.advance_clock(now)
    }

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<Vec<Effect>> {
        let mut arp = ArpPdu::try_from(bytes.as_slice())?;
        match arp.op {
            ArpOp::ArpRequest => {
                if arp.target_ip_addr != self.options.my_ipv4_addr {
                    return Err(Fail::Ignored {});
                }

                arp.op = ArpOp::ArpReply;
                arp.target_link_addr = self.options.my_link_addr;
                swap(&mut arp.sender_ip_addr, &mut arp.target_ip_addr);
                swap(&mut arp.sender_link_addr, &mut arp.target_link_addr);

                let ether2_header = ethernet2::Header {
                    dest_addr: arp.target_link_addr,
                    src_addr: arp.sender_link_addr,
                    ether_type: ethernet2::EtherType::Arp,
                };

                let mut packet = Vec::with_capacity(
                    ArpPdu::size() + ethernet2::Header::size(),
                );
                ether2_header.write(&mut packet)?;
                arp.write(&mut packet)?;
                Ok(vec![Effect::Transmit(packet)])
            }
            _ => Err(Fail::Unsupported {}),
        }
    }
}
