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
/// Seed for the random number generator.
#[cfg(debug_assertions)]
const RNG_SEED: u64 = 42;
#[cfg(not(debug_assertions))]
const DEFAULT_ID: u64 = 500;

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Id64Map<E, I> {
    #[allow(unused)]
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

    #[cfg(debug_assertions)]
    fn generate_id(&mut self) -> u64 {
        self.rng.next_u64()
    }

    #[cfg(not(debug_assertions))]
    fn generate_id(&mut self) -> u64 {
        self.current_id = self.current_id.wrapping_add(1);
        self.current_id
    }
}

impl<E: Eq + Hash + From<u32> + Into<u32> + Copy, I: From<u32> + Into<u32> + Copy> Id32Map<E, I> {
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

    #[cfg(debug_assertions)]
    fn generate_id(&mut self) -> u32 {
        self.rng.next_u32()
    }

    #[cfg(not(debug_assertions))]
    fn generate_id(&mut self) -> u32 {
        self.current_id = self.current_id.wrapping_add(1);
        self.current_id
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Default for Id64Map<E, I> {
    fn default() -> Self {
        Self {
            ids: HashMap::<E, I>::with_capacity(DEFAULT_SIZE),
            #[cfg(debug_assertions)]
            rng: SmallRng::seed_from_u64(RNG_SEED),
            #[cfg(not(debug_assertions))]
            current_id: DEFAULT_ID,
        }
    }
}

impl<E: Eq + Hash + From<u32> + Into<u32> + Copy, I: From<u32> + Into<u32> + Copy> Default for Id32Map<E, I> {
    fn default() -> Self {
        Self {
            ids: HashMap::<E, I>::with_capacity(DEFAULT_SIZE),
            #[cfg(debug_assertions)]
            rng: SmallRng::seed_from_u64(RNG_SEED),
            #[cfg(not(debug_assertions))]
            current_id: DEFAULT_ID as u32,
        }
    }
}
