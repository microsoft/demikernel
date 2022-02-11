// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::LinuxRuntime;
use ::rand::{
    distributions::Standard,
    prelude::Distribution,
    seq::SliceRandom,
    Rng,
};
use ::runtime::utils::UtilsRuntime;

//==============================================================================
// Trait Implementations
//==============================================================================

impl UtilsRuntime for LinuxRuntime {
    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        let mut inner = self.inner.borrow_mut();
        inner.rng.gen()
    }

    fn rng_shuffle<T>(&self, slice: &mut [T]) {
        let mut inner = self.inner.borrow_mut();
        slice.shuffle(&mut inner.rng);
    }
}
