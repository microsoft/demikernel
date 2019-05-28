use crate::{prelude::*, rand::Rng};
use rand_core::SeedableRng;
use std::collections::VecDeque;

pub struct RuntimeState {
    options: Options,
    rng: Rng,
    effects: VecDeque<Effect>,
}

impl RuntimeState {
    pub fn from_options(options: Options) -> RuntimeState {
        let seed = options.rng_seed;
        RuntimeState {
            options,
            rng: Rng::from_seed(seed),
            effects: VecDeque::new(),
        }
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    pub fn effects(&mut self) -> &mut VecDeque<Effect> {
        &mut self.effects
    }
}
