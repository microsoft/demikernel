use crate::protocols::arp;
use crate::rand::Seed;
use eui48::MacAddress;

#[derive(Default)]
pub struct Options {
    pub my_link_addr: MacAddress,
    pub rng_seed: Seed,
    pub arp: arp::Options,
}
