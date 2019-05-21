use crate::prelude::*;
use crate::protocols::ethernet2::EtherType;
use std::time::Instant;
use std::convert::{TryFrom, TryInto};
use super::cache::ArpCache;
use super::message::{ArpMessage, ArpOp};
use std::mem::swap;
use etherparse::PacketBuilder;

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

    pub fn receive(&mut self, packet: Packet) -> Result<Vec<Effect>> {
        let ether2_header = packet.parse_ether2_header()?;
        assert_eq!(EtherType::Arp as u16, ether2_header.ether_type);
        let mut msg = ArpMessage::try_from(packet.payload()?)?;
        match msg.op {
            ArpOp::ArpRequest => {
                if msg.target_ip_addr != self.options.my_ipv4_addr {
                    return Err(Fail::Ignored {});
                }

                msg.op = ArpOp::ArpReply;
                msg.target_link_addr = self.options.my_link_addr;
                swap(&mut msg.sender_ip_addr, &mut msg.target_ip_addr);
                swap(&mut msg.sender_link_addr, &mut msg.target_link_addr);
                let packet = PacketBuilder::ethernet2(self.options.my_link_addr.to_array(), msg.target_link_addr.to_array());
                let payload = msg.try_into()?;
                let mut bytes = Vec::with_capacity(packet.size(payload.len());
                packet.write(&mut bytes, &payload)?;
                return Ok(vec![Effect::Transmit(bytes)]);
            },
            _ => return Err(Fail::Unsupported {}),
        }

        Ok(vec![])
    }
}
