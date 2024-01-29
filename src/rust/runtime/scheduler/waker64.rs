// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use ::std::{
    cell::UnsafeCell,
    mem,
};

//==============================================================================
// Constants
//==============================================================================

/// Log2 of [WAKER_BIT_LENGTH].
pub const WAKER_BIT_LENGTH_SHIFT: usize = 6;

/// Number of Bits in a [Waker64]
pub const WAKER_BIT_LENGTH: usize = 1 << WAKER_BIT_LENGTH_SHIFT;

//==============================================================================
// Structures
//==============================================================================

/// 64-Bit Waker
pub struct Waker64(UnsafeCell<u64>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for 64-Bit Wakers
impl Waker64 {
    /// Creates a 64-Bit Waker from `val`.
    pub fn new(val: u64) -> Self {
        Waker64(UnsafeCell::new(val))
    }

    /// Applies the OR operator between `val` and the target [Waker64].
    /// The resulting value is stored back in the target [Waker64].
    pub fn fetch_or(&self, val: u64) {
        let s = unsafe { &mut *self.0.get() };
        *s |= val;
    }

    /// Applies the AND operator between `val` and the target [Waker64].
    /// The resulting value is stored back in the target [Waker64].
    pub fn fetch_and(&self, val: u64) {
        let s = unsafe { &mut *self.0.get() };
        *s &= val;
    }

    /// Applies the ADD operator between `val` and the target [Waker64].
    /// The resulting value is stored back in the target [Waker64] and the old
    /// value is returned.
    pub fn fetch_add(&self, val: u64) -> u64 {
        let s = unsafe { &mut *self.0.get() };
        let old = *s;
        *s += val;
        old
    }

    /// Applies the SUB operator between `val` and the target [Waker64].
    /// The resulting value is stored back in the target [Waker64].
    /// If the operation does not overflow, the old value is returned.
    /// Otherwise, `None` is returned instead.
    pub fn fetch_sub(&self, val: u64) -> Option<u64> {
        let s: &mut u64 = unsafe { &mut *self.0.get() };
        let old: u64 = *s;
        if val > *s {
            return None;
        }
        *s -= val;
        Some(old)
    }

    #[allow(unused)]
    /// Returns the value stored in the the target [Waker64].
    pub fn load(&self) -> u64 {
        let s = unsafe { &mut *self.0.get() };
        *s
    }

    /// Replaces the value stored in the the target [Waker64] by `val`.
    pub fn swap(&self, val: u64) -> u64 {
        let s = unsafe { &mut *self.0.get() };
        mem::replace(s, val)
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Sync Trait Implementation for 64-Bit Wakers
unsafe impl Sync for Waker64 {}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use super::Waker64;
    use ::rand::Rng;
    use ::test::{
        black_box,
        Bencher,
    };

    #[bench]
    fn bench_fetch_and(b: &mut Bencher) {
        let x: u64 = rand::thread_rng().gen_range(0..64);
        let w64: Waker64 = Waker64::new(0);

        b.iter(|| {
            let val: u64 = black_box(x);
            w64.fetch_and(val);
        });
    }

    #[bench]
    fn bench_fetch_or(b: &mut Bencher) {
        let x: u64 = rand::thread_rng().gen_range(0..64);
        let w64: Waker64 = Waker64::new(0);

        b.iter(|| {
            let val: u64 = black_box(x);
            w64.fetch_or(val);
        });
    }

    #[bench]
    fn bench_fetch_add(b: &mut Bencher) {
        let x: u64 = rand::thread_rng().gen_range(0..64);
        let w64: Waker64 = Waker64::new(0);

        b.iter(|| {
            let val: u64 = black_box(x);
            w64.fetch_add(val);
        });
    }

    #[bench]
    fn bench_fetch_sub(b: &mut Bencher) {
        let x: u64 = rand::thread_rng().gen_range(0..64);

        b.iter(|| {
            let val: u64 = black_box(x);
            let w64: Waker64 = Waker64::new(64);
            w64.fetch_sub(val).expect("fetch_sub() overflowed");
        });
    }

    #[bench]
    fn bench_load(b: &mut Bencher) {
        let x: u64 = rand::thread_rng().gen_range(0..64);
        let w64: Waker64 = Waker64::new(x);

        b.iter(|| {
            let val: u64 = w64.load();
            black_box(val);
        });
    }

    #[bench]
    fn bench_swap(b: &mut Bencher) {
        let x: u64 = rand::thread_rng().gen_range(0..64);
        let w64: Waker64 = Waker64::new(0);

        b.iter(|| {
            let val: u64 = black_box(x);
            let oldval: u64 = w64.swap(val);
            black_box(oldval);
        });
    }
}
