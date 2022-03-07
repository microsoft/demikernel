// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::DPDKRuntime;
use ::rand::{
    distributions::{
        Distribution,
        Standard,
    },
    seq::SliceRandom,
    Rng,
};
use ::runtime::utils::UtilsRuntime;

//==============================================================================
// Trait Implementations
//==============================================================================

/// Utils Runtime Trait Implementation for DPDK Runtime
impl UtilsRuntime for DPDKRuntime {
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
