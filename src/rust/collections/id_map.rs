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
    convert::From,
    hash::Hash,
};

//======================================================================================================================
// Constants
//======================================================================================================================

/// This flag controls whether we actually use a mapping or just directly expose internal IDs.
const DIRECT_MAPPING: bool = false;
/// Seed for the random number generator used to generate tokens.
/// This value was chosen arbitrarily.
#[cfg(debug_assertions)]
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
        if !DIRECT_MAPPING {
            self.ids.insert(external_id, internal_id)
        } else {
            None
        }
    }

    /// Remove a mapping between a specificed external and internal id. If the mapping exists, then return the internal
    /// id mapped to the external id. If we are using a direct mapping, then this is a no op.
    pub fn remove(&mut self, external_id: &E) -> Option<I> {
        if !DIRECT_MAPPING {
            self.ids.remove(external_id)
        } else {
            None
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

        // Otherwise, allocate a new external id.
        for _ in 0..MAX_RETRIES_ID_ALLOC {
            let external_id: E = E::from(self.rng.next_u64());
            if !self.ids.contains_key(&external_id) {
                self.ids.insert(external_id, internal_id);
                return external_id;
            }
        }
        panic!("Could not find a valid task id");
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

/// A default implementation for the external to internal id map.
impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Default for IdMap<E, I> {
    fn default() -> Self {
        Self {
            ids: HashMap::<E, I>::default(),
            #[cfg(debug_assertions)]
            rng: SmallRng::seed_from_u64(SCHEDULER_SEED),
            #[cfg(not(debug_assertions))]
            rng: SmallRng::from_entropy(),
        }
    }
}
