// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::runtime::scheduler::waker64::{
    Waker64,
    WAKER_BIT_LENGTH,
};

//==============================================================================
// Constants
//==============================================================================

/// Size of Pages (in bytes)
pub const WAKER_PAGE_SIZE: usize = 64;

//==============================================================================
// Structures
//==============================================================================

/// Waker Page
///
/// This structure holds the status of multiple futures in the scheduler. It is
/// composed by 3 bitmaps, each of which having the ith bit to represent some
/// state for the ith future.
///
/// The number of bytes in this structure should match the number of bits in a
/// [Waker64]. Furthermore, the structure should be aligned in memory with its
/// own size. We rely on these two properties to distribute raw pointers to the
/// scheduler, so that it may cast back a raw pointer and operate on a specific
/// future whenever needed.
#[repr(align(64))]
pub struct WakerPage {
    /// Reference count for the page.
    refcount: Waker64,
    /// Flags wether or not a given future has been notified.
    notified: Waker64,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Waker Page
impl WakerPage {
    /// Sets the notification flag for the `ix` future in the target [WakerPage].
    pub fn notify(&self, ix: usize) {
        debug_assert!(ix < WAKER_BIT_LENGTH);
        self.notified.fetch_or(1 << ix);
    }

    /// Takes out notification flags in the target [WakerPage].
    /// Notification flags are reset after this operation.
    pub fn take_notified(&self) -> u64 {
        // Unset all completed bits, since spurious notifications for completed
        // futures would lead us to poll them after completion.
        self.notified.swap(0)
    }

    /// Resets all flags in the target [WakerPage].
    /// The reference count for the target page is reset to one.
    pub fn reset(&mut self) {
        self.refcount.swap(1);
        self.notified.swap(0);
    }

    /// Initialize flags for the `ix` future in the target [WakerPage].
    /// Notification and completed flags are reset after this operation.
    pub fn initialize(&self, ix: usize) {
        debug_assert!(ix < WAKER_BIT_LENGTH);
        self.notified.fetch_or(1 << ix);
    }

    /// Clears flags for the `ix` future in the target [WakerPage]
    /// The reference count for the target page is left unmodified.
    pub fn clear(&self, ix: usize) {
        debug_assert!(ix < WAKER_BIT_LENGTH);
        let mask: u64 = !(1 << ix);
        self.notified.fetch_and(mask);
    }

    /// Increments the reference count of the target [WakerPage].
    /// The old reference count is returned.
    pub fn refcount_inc(&self) -> u64 {
        self.refcount.fetch_add(1)
    }

    /// Decrements the reference count of the target [WakerPage].
    /// Upon successful completion, the old reference count is returned.
    /// Otherwise, `None` is returned instead.
    pub fn refcount_dec(&self) -> Option<u64> {
        self.refcount.fetch_sub(1)
    }

    /// Gets the reference count of the target [WakerPage].
    #[cfg(test)]
    pub fn refcount_get(&self) -> u64 {
        self.refcount.load()
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Default Trait Implementation for Waker Pages
impl Default for WakerPage {
    fn default() -> Self {
        Self {
            refcount: Waker64::new(1),
            notified: Waker64::new(0),
        }
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use super::{
        WakerPage,
        WAKER_BIT_LENGTH,
        WAKER_PAGE_SIZE,
    };
    use ::anyhow::Result;
    use ::rand::Rng;
    use ::std::mem;
    use ::test::{
        black_box,
        Bencher,
    };

    #[test]
    fn test_sizes() -> Result<()> {
        crate::ensure_eq!(WAKER_PAGE_SIZE, WAKER_BIT_LENGTH);
        crate::ensure_eq!(mem::size_of::<WakerPage>(), WAKER_PAGE_SIZE);
        Ok(())
    }

    #[bench]
    fn bench_notify(b: &mut Bencher) {
        let pg: WakerPage = WakerPage::default();
        let x: usize = rand::thread_rng().gen_range(0..WAKER_BIT_LENGTH);

        b.iter(|| {
            let ix: usize = black_box(x);
            pg.notify(ix);
        });
    }

    #[bench]
    fn bench_take_notified(b: &mut Bencher) {
        let pg: WakerPage = WakerPage::default();

        // Initialize 8 random bits.
        for _ in 0..8 {
            let ix: usize = rand::thread_rng().gen_range(0..WAKER_BIT_LENGTH);
            pg.initialize(ix);
        }

        b.iter(|| {
            let x: u64 = pg.take_notified();
            black_box(x);
        });
    }
}
