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

    pub fn poll(&mut self) -> Option<Effect> {
        self.effects.pop_front()
    }

    pub fn emit(&mut self, effect: Effect) {
        self.effects.push_back(effect)
    }
}
