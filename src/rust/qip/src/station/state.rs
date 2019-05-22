use crate::prelude::*;
use crate::protocols::arp;
use crate::protocols::ethernet2;
use crate::rand::Rng;
use eui48::MacAddress;
use rand_core::SeedableRng;
use std::convert::TryFrom;
use std::time::Instant;

pub struct State {
    options: Options,
    rng: Rng,
    arp: arp::State,
}

impl State {
    pub fn from_options(
        options: Options,
        now: Instant,
    ) -> State {
        let seed = options.rng_seed;
        let arp = arp::State::from_options(&options, now);
        State {
            options,
            rng: Rng::from_seed(seed),
            arp,
        }
    }

    pub fn advance_clock(&mut self, now: Instant) {
        self.arp.advance_clock(now)
    }

    pub fn receive(&mut self, bytes: Vec<u8>) -> Result<Vec<Effect>> {
        let frame = ethernet2::Frame::try_from(bytes)?;

        let dest_addr = frame.header().dest_addr;
        if self.options.my_link_addr != dest_addr
            && MacAddress::broadcast() != dest_addr
        {
            return Err(Fail::Misdelivered {});
        }

        match frame.header().ether_type {
            ethernet2::EtherType::Arp => {
                let payload = frame.payload().to_vec();
                self.arp.receive(payload)
            }
        }
    }
}
