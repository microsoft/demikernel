use crate::prelude::*;
use crate::r#async;
use crate::rand::Rng;
use rand_core::SeedableRng;

pub struct RuntimeState<'a> {
    options: Options,
    rng: Rng,
    r#async: r#async::State<'a>,
}

impl<'a> RuntimeState<'a> {
    pub fn from_options(options: Options) -> RuntimeState<'a> {
        let seed = options.rng_seed;
        RuntimeState {
            options,
            rng: Rng::from_seed(seed),
            r#async: r#async::State::new(),
        }
    }

    pub fn options(&self) -> &Options {
        &self.options
    }
}
