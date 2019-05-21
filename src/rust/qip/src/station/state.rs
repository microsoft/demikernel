use crate::prelude::*;
use crate::protocols::ethernet2::EtherType;
use crate::rand::Rng;
use crate::sync::{Arc, Mutex};
use eui48::MacAddress;
use rand_core::SeedableRng;
use std::time::Instant;

pub use crate::protocols::arp;

const ETHER_TYPE_ARP: u16 = EtherType::Arp as u16;

pub struct State {
    options: Options,
    rng: Rng,
    shared: Arc<Mutex<SharedState>>,
}

impl State {
    pub fn from_options(
        options: Options,
        shared: Arc<Mutex<SharedState>>,
    ) -> State {
        let seed = options.rng_seed;
        State {
            options,
            rng: Rng::from_seed(seed),
            shared,
        }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        let mut shared = self.shared.lock();
        shared.arp.advance_clock(now)
    }

    pub fn receive(&mut self, packet: Vec<u8>) -> Result<Vec<Effect>> {
        let packet = Packet::from(packet);
        let ether2_header = packet.parse_ether2_header()?;

        let dest_link_addr =
            MacAddress::from_bytes(&ether2_header.destination).unwrap();
        if self.options.my_link_addr != dest_link_addr
            && MacAddress::broadcast() != dest_link_addr
        {
            return Err(Fail::Misdelivered {});
        }

        match ether2_header.ether_type {
            ETHER_TYPE_ARP => {
                let mut shared = self.shared.lock();
                shared.arp.receive(packet)
            }
            _ => Err(Fail::Unsupported {}),
        }
    }
}

pub struct SharedState {
    arp: arp::State,
}

impl SharedState {
    pub fn from_options(
        options: &Options,
        now: Instant,
    ) -> Result<SharedState> {
        Ok(SharedState {
            arp: arp::State::from_options(options, now),
        })
    }
}
