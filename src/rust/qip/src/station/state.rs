use crate::prelude::*;
use crate::protocols::arp;
use crate::protocols::ethernet2;
use crate::rand::Rng;
use crate::sync::{Arc, Mutex};
use eui48::MacAddress;
use rand_core::SeedableRng;
use std::convert::TryFrom;
use std::time::Instant;

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
                let mut shared = self.shared.lock();
                shared.arp.receive(payload)
            }
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
