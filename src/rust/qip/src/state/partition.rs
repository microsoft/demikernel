use crate::rand::{Rng, Seed};
use rand_core::SeedableRng;

pub struct Partition {
    seed: Seed,
    pub rng: Rng,
}

impl Partition {
    pub fn new() -> Partition {
        let seed = Seed::default();
        Partition {
            seed,
            rng: Rng::from_seed(seed),
        }
    }

    pub fn service(&self) {}
}
