// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT license.

//======================================================================================================================
// Constants
//======================================================================================================================

// Value that we use to offset direct mapping for 32 bit ids. This number is chosen to avoid collisions with a small
// default set of POSIX file descriptors.
const ID_OFFSET: u32 = 500;

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Id64Map<E, I> {
    #[inline(always)]
    #[allow(unused)]
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

    fn generate_id(&mut self) -> u64 {
        self.current_id = self.current_id.wrapping_add(1);
        self.current_id
    }

    fn mask_id(external_id: &E) -> I {
        let masked_id: u32 = <E as Into<u64>>::into(*external_id) as u32;
        <I as From<u64>>::from(masked_id as u64)
    }
}

impl<E: Eq + Hash + From<u32> + Into<u32> + Copy, I: From<u32> + Into<u32> + Copy> Id32Map<E, I> {
    #[inline(always)]
    pub fn get(&self, external_id: &E) -> Option<I> {
        Some(Into::<I>::into(Into::<u32>::into(*external_id).checked_sub(ID_OFFSET)?))
    }

    #[inline(always)]
    pub fn remove(&mut self, external_id: &E) -> Option<I> {
        Some(Into::<I>::into(Into::<u32>::into(*external_id).checked_sub(ID_OFFSET)?))
    }

    #[inline(always)]
    pub fn insert_with_new_id(&mut self, internal_id: I) -> Option<E> {
        match TryInto::<u32>::try_into(Into::<u32>::into(internal_id)) {
            Ok(id) => Some(
                id.checked_add(ID_OFFSET)
                    .expect("should not run out of 32-bit id space")
                    .into(),
            ),
            Err(_) => None,
        }
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> Default for Id64Map<E, I> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
            current_id: 1,
        }
    }
}

impl<E: Eq + Hash + From<u32> + Into<u32> + Copy, I: From<u32> + Into<u32> + Copy> Default for Id32Map<E, I> {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}
