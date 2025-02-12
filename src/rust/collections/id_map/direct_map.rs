// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> IdMap<E, I> {
    #[inline(always)]
    pub fn get(&self, external_id: &E) -> Option<I> {
        Some(Self::mask_id(external_id))
    }

    #[inline(always)]
    pub fn remove(&mut self, external_id: &E) -> Option<I> {
        Some(Self::mask_id(external_id))
    }

    #[inline(always)]
    pub fn insert_with_new_id(&mut self, internal_id: I) -> Option<E> {
        let higher_order_bits: u64 = self.generate_id() as u64;
        // Use random number for higher order bits and the offset for lower order bits.
        let external_id: u64 = higher_order_bits << 32 | <I as Into<u64>>::into(internal_id);
        Some(external_id.into())
    }

    fn mask_id(external_id: &E) -> I {
        let masked_id: u32 = <E as Into<u64>>::into(*external_id) as u32;
        <I as From<u64>>::from(masked_id as u64)
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Default for IdMap<E, I> {
    fn default() -> Self {
        Self {
            // Don't need to pre-allocate, the overhead is a 6ns on the scheduler insert benchmark.
            _phantom: PhantomData,
            rng: SmallRng::seed_from_u64(ID_SEED),
            last_id: 1,
            current_id: 2,
        }
    }
}
