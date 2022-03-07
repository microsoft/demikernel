// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

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

/// Utilities Runtime Trait Implementation for Linux Runtime
impl UtilsRuntime for LinuxRuntime {
    /// Returns a random value supporting the [Standard] distribution.
    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        self.rng.borrow_mut().gen()
    }

    /// Shuffles a mutable slice in place.
    fn rng_shuffle<T>(&self, slice: &mut [T]) {
        let rng = self.rng.borrow_mut();
        slice.shuffle(&mut rng.to_owned());
    }
}
