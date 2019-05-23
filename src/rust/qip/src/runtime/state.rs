use crate::prelude::*;
use crate::rand::Rng;
use rand_core::SeedableRng;

pub struct RuntimeState {
    options: Options,
    rng: Rng,
}

impl RuntimeState {
    pub fn from_options(options: Options) -> RuntimeState {
        let seed = options.rng_seed;
        RuntimeState {
            options,
            rng: Rng::from_seed(seed),
        }
    }

    pub fn options(&self) -> &Options {
        &self.options
    }
}
