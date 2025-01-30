// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::rand::{rngs::SmallRng, RngCore, SeedableRng};
#[cfg(not(feature = "direct-mapping"))]
use ::std::collections::HashMap;
use ::std::hash::Hash;
#[cfg(feature = "direct-mapping")]
use ::std::marker::PhantomData;

const ID_SEED: u64 = 42;

#[cfg(debug_assertions)]
const RANDOMIZE: bool = true;
#[cfg(not(debug_assertions))]
const RANDOMIZE: bool = false;

pub struct IdMap<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> {
    /// Map between external and internal ids.
    #[cfg(feature = "direct-mapping")]
    num_entries: usize,
    #[cfg(feature = "direct-mapping")]
    _phantom: PhantomData<(E, I)>,
    #[cfg(not(feature = "direct-mapping"))]
    ids: HashMap<E, I>,
    /// Small random number generator for external ids.
    rng: SmallRng,
    /// For non-random id generation, we keep the last 2 id numbers for a Fibonacci calculation.
    last_id: u64,
    current_id: u64,
}

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> IdMap<E, I> {
    fn generate_id(&mut self) -> u64 {
        if RANDOMIZE {
            // Otherwise, allocate a new external id.
            self.rng.next_u64()
        } else {
            // Use a Fibonacci sequence.
            let id: u64 = self.current_id;
            // Roll around.
            self.current_id = if self.current_id < u64::MAX - self.last_id {
                self.current_id + self.last_id
            } else {
                self.last_id - (u64::MAX - self.current_id)
            };
            self.last_id = id;

            id
        }
    }
}

#[cfg(feature = "direct-mapping")]
include!("direct_map.rs");
#[cfg(not(feature = "direct-mapping"))]
include!("indirect_map.rs");
