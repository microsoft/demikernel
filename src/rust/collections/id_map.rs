// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::rand::{
    rngs::SmallRng,
    RngCore,
    SeedableRng,
};
use ::std::{
    collections::HashMap,
    hash::Hash,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// Performance note: These two flags were benchmarked with the scheduler insert benchmark on a
/// release build and have the following impact on performance. The number is the average of 5 runs of the test and
/// performed on an Azure Standard D16ds v4 (16 vcpus, 64 GiB memory) VM running Linux (ubuntu 22.04).
/// Direct vs indirect mapping: 152 for direct, indirect is below.
/// Randomize vs non random: 332ns vs 154ns

/// This flag controls whether we actually use a mapping or just directly expose internal IDs.
/// We should eventually set this using an environment variable.
const DIRECT_MAPPING: bool = true;
/// This flag controls how the ids are allocated, either randomly or in a Fibonacci sequence.
#[cfg(debug_assertions)]
const RANDOMIZE: bool = true;
#[cfg(not(debug_assertions))]
const RANDOMIZE: bool = false;

/// Arbitrary size chosen to pre-allocate the hashmap. This improves performance by 6ns on average on our scheduler
/// insert benchmark.
const DEFAULT_SIZE: usize = 1024;

/// Seed for the random number generator used to generate tokens.
/// This value was chosen arbitrarily.
const SCHEDULER_SEED: u64 = 42;
const MAX_RETRIES_ID_ALLOC: usize = 500;

//======================================================================================================================
// Structures
//======================================================================================================================

/// This data structure is a general-purpose map for obfuscating ids from external modules. It takes an external id type
/// and an internal id type and translates between the two. The ID types must be basic types that can be converted back
/// and forth between u64 and therefore each other.
pub struct IdMap<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> {
    /// Map between external and internal ids.
    ids: HashMap<E, I>,
    /// Small random number generator for external ids.
    rng: SmallRng,
    /// For non-random id generation, we keep the last 2 id numbers for a Fibonacci calculation.
    last_id: u64,
    current_id: u64,
    #[cfg(test)]
    /// For direct mapping, we keep track of the total number of mappings with a counter.
    num_mappings: usize,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> IdMap<E, I> {
    /// Retrieve a mapping for this external id if it exists. If we are using a direct mapping, this operation always
    /// succeeds, so DO NOT use this function to check for the existance of a particular key. We expect the user to use
    /// nother data structure for validity.
    pub fn get(&self, external_id: &E) -> Option<I> {
        // If we are not obfuscating ids, just return the external id.
        if DIRECT_MAPPING {
            Some(<E as Into<u64>>::into(*external_id).into())
        } else {
            self.ids.get(external_id).copied()
        }
    }

    #[allow(dead_code)]
    /// Insert a mapping between a specified external and internal id. If we are using a direct mapping,
    /// then this is a no op.
    pub fn insert(&mut self, external_id: E, internal_id: I) -> Option<I> {
        if DIRECT_MAPPING {
            #[cfg(test)]
            {
                self.num_mappings = self.num_mappings + 1;
            }
            None
        } else {
            self.ids.insert(external_id, internal_id)
        }
    }

    /// Remove a mapping between a specificed external and internal id. If the mapping exists, then return the internal
    /// id mapped to the external id. If we are using a direct mapping, then this is a no op.
    pub fn remove(&mut self, external_id: &E) -> Option<I> {
        if DIRECT_MAPPING {
            #[cfg(test)]
            {
                self.num_mappings = self.num_mappings - 1;
            }
            Some(<E as Into<u64>>::into(*external_id).into())
        } else {
            self.ids.remove(external_id)
        }
    }

    /// Generate a new id and insert the mapping to the internal id. If the id is currently in use, keep generating
    /// until we find an unused id (up to a maximum number of tries). If we are using a direct mapping, then just
    /// return the internal id without generating a new id or inserting a mapping.
    pub fn insert_with_new_id(&mut self, internal_id: I) -> E {
        // If we are not obfuscating ids, just return the external id.
        if DIRECT_MAPPING {
            return E::from(internal_id.into());
        }

        if RANDOMIZE {
            // Otherwise, allocate a new external id.
            for _ in 0..MAX_RETRIES_ID_ALLOC {
                let external_id: E = E::from(self.rng.next_u64());
                if !self.ids.contains_key(&external_id) {
                    self.ids.insert(external_id, internal_id);
                    return external_id;
                }
            }
            panic!("Could not find a valid task id");
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
            let external_id: E = E::from(id);
            if self.ids.insert(external_id, internal_id).is_some() {
                panic!("Should not have a previous task with this id");
            }
            external_id
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        if DIRECT_MAPPING {
            self.num_mappings
        } else {
            self.ids.len()
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// A default implementation for the external to internal id map.
impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Default for IdMap<E, I> {
    fn default() -> Self {
        Self {
            // Don't need to pre-allocate, the overhead is a 6ns on the scheduler insert benchmark.
            ids: HashMap::<E, I>::with_capacity(DEFAULT_SIZE),
            rng: SmallRng::seed_from_u64(SCHEDULER_SEED),
            last_id: 1,
            current_id: 2,
            #[cfg(test)]
            num_mappings: 0,
        }
    }
}
