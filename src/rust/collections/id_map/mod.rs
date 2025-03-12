// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

#[cfg(all(not(feature = "direct-mapping"), debug_assertions))]
use ::rand::{rngs::SmallRng, RngCore, SeedableRng};
#[cfg(not(feature = "direct-mapping"))]
use ::std::collections::HashMap;
use ::std::hash::Hash;
#[cfg(feature = "direct-mapping")]
use ::std::marker::PhantomData;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A 64-bit mapping table for IDs. This table uses monotonically increasing ids that directly indexes into the slab
/// offset in "direct-mapping" mode. With "direct-mapping" off, the table uses random ids with a hashmap for
/// dereferencing.
#[cfg(feature = "direct-mapping")]
pub struct Id64Map<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> {
    _phantom: PhantomData<(E, I)>,
    current_id: u64,
}

#[cfg(not(feature = "direct-mapping"))]
pub struct Id64Map<E: Eq + Hash + From<u64> + Into<u64> + Copy, I: From<u64> + Into<u64> + Copy> {
    /// Map between external and internal ids.
    ids: HashMap<E, I>,
    /// Small random number generator for external ids.
    #[cfg(debug_assertions)]
    rng: SmallRng,
    #[cfg(not(debug_assertions))]
    current_id: u64,
}

/// Same as the 64-bit table but a 32-bit to 32-bit id mapping table for ids that will be externalized as uints.
#[cfg(feature = "direct-mapping")]
pub struct Id32Map<E: Eq + Hash + From<u32> + Into<u32> + Copy, I: From<u32> + Into<u32> + Copy> {
    _phantom: PhantomData<(E, I)>,
}

#[cfg(not(feature = "direct-mapping"))]
pub struct Id32Map<E: Eq + Hash + From<u32> + Into<u32> + Copy, I: From<u32> + Into<u32> + Copy> {
    /// Map between external and internal ids.
    ids: HashMap<E, I>,
    /// Small random number generator for external ids.
    #[cfg(debug_assertions)]
    rng: SmallRng,
    #[cfg(not(debug_assertions))]
    current_id: u32,
}

#[cfg(feature = "direct-mapping")]
include!("direct_map.rs");
#[cfg(not(feature = "direct-mapping"))]
include!("indirect_map.rs");
