use crate::rand::{Rng, Seed};
use rand_core::SeedableRng;

pub struct PartitionedState {
    seed: Seed,
    pub rng: Rng,
}

impl PartitionedState {
    pub fn new() -> PartitionedState {
        let seed = Seed::default();
        PartitionedState {
            seed,
            rng: Rng::from_seed(seed),
        }
    }

    pub fn service(&self) {}
}
