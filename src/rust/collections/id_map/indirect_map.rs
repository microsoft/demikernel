// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Constants
//======================================================================================================================

/// Arbitrary size chosen to pre-allocate the hashmap. This improves performance by 6ns on average on our scheduler
/// insert benchmark.
const DEFAULT_SIZE: usize = 1024;
/// An arbitrary upper bound to find a unique id.
const MAX_RETRIES_ID_ALLOC: usize = 500;

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> IdMap<E, I> {
    pub fn get(&self, external_id: &E) -> Option<I> {
        self.ids.get(external_id).copied()
    }

    /// Remove a mapping between a specificed external and internal id. If the mapping exists, then return the internal
    /// id mapped to the external id.
    pub fn remove(&mut self, external_id: &E) -> Option<I> {
        self.ids.remove(external_id)
    }

    /// Generate a new id and insert the mapping to the internal id. If the id is currently in use, keep generating
    /// until we find an unused id (up to a maximum number of tries).
    pub fn insert_with_new_id(&mut self, internal_id: I) -> Option<E> {
        // Otherwise, allocate a new external id.
        for _ in 0..MAX_RETRIES_ID_ALLOC {
            let external_id: E = E::from(self.generate_id());
            if !self.ids.contains_key(&external_id) {
                self.ids.insert(external_id, internal_id);
                return Some(external_id);
            }
        }
        warn!("Could not find a valid task id");
        None
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Default for IdMap<E, I> {
    fn default() -> Self {
        Self {
            // Don't need to pre-allocate, the overhead is a 6ns on the scheduler insert benchmark.
            ids: HashMap::<E, I>::with_capacity(DEFAULT_SIZE),
            rng: SmallRng::seed_from_u64(ID_SEED),
            last_id: 1,
            current_id: 2,
        }
    }
}
