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
    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        let mut self_ = self.inner.borrow_mut();
        self_.rng.gen()
    }

    fn rng_shuffle<T>(&self, slice: &mut [T]) {
        let mut inner = self.inner.borrow_mut();
        slice.shuffle(&mut inner.rng);
    }
}
