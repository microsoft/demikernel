// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::IoUringRuntime;
use ::rand::{
    distributions::Standard,
    prelude::Distribution,
};
use ::runtime::utils::UtilsRuntime;

//==============================================================================
// Trait Implementations
//==============================================================================

/// Utilities runtime trait implementation for I/O User ring runtime.
impl UtilsRuntime for IoUringRuntime {
    // TODO: Rely on a default implementation for this.
    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        unreachable!()
    }

    // TODO: Rely on a default implementation for this.
    fn rng_shuffle<T>(&self, _slice: &mut [T]) {
        unreachable!()
    }
}
