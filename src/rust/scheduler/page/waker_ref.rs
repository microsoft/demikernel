// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    page::WakerPage,
    WakerPageRef,
    WAKER_PAGE_SIZE,
};
use ::std::{
    mem,
    ptr::NonNull,
    task::{
        RawWaker,
        RawWakerVTable,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Waker Reference
///
/// This reference is a representation for the status of a particular task
/// stored in a [WakerPage].
#[repr(transparent)]
pub struct WakerRef(NonNull<u8>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Waker References
impl WakerRef {
    pub fn new(raw_page_ref: NonNull<u8>) -> Self {
        Self(raw_page_ref)
    }

    /// Casts the target [WakerRef] back into reference to a [WakerPage] plus an
    /// offset indicating the target task in the latter structure.
    ///
    /// For more information on this hack see comments on [crate::page::WakerPageRef].
    fn base_ptr(&self) -> (NonNull<WakerPage>, usize) {
        let ptr: *mut u8 = self.0.as_ptr();
        let forward_offset: usize = ptr.align_offset(WAKER_PAGE_SIZE);
        let mut base_ptr: *mut u8 = ptr;
        let mut offset: usize = 0;
        if forward_offset != 0 {
            offset = WAKER_PAGE_SIZE - forward_offset;
            base_ptr = ptr.wrapping_sub(offset);
        }
        unsafe { (NonNull::new_unchecked(base_ptr).cast(), offset) }
    }

    /// Sets the notification flag for the task that associated with the target [WakerRef].
    fn wake_by_ref(&self) {
        let (base_ptr, ix): (NonNull<WakerPage>, usize) = self.base_ptr();
        let base: &WakerPage = unsafe { &*base_ptr.as_ptr() };
        base.notify(ix);
    }

    /// Sets the notification flag for the task that is associated with the target [WakerRef].
    fn wake(self) {
        self.wake_by_ref()
    }

    /// Gets the reference count of the target [WakerRef].
    #[cfg(test)]
    pub fn refcount_get(&self) -> u64 {
        let (base_ptr, _): (NonNull<WakerPage>, _) = self.base_ptr();
        unsafe { base_ptr.as_ref().refcount_get() }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Clone Trait Implementation for Waker References
impl Clone for WakerRef {
    fn clone(&self) -> Self {
        let (base_ptr, _): (NonNull<WakerPage>, _) = self.base_ptr();
        let p: WakerPageRef = WakerPageRef::new(base_ptr);
        // Increment reference count.
        mem::forget(p.clone());
        // This is not a double increment.
        mem::forget(p);
        WakerRef(self.0)
    }
}

/// Drop Trait Implementation for Waker References
impl Drop for WakerRef {
    fn drop(&mut self) {
        let (base_ptr, _) = self.base_ptr();
        // Decrement the refcount.
        drop(WakerPageRef::new(base_ptr));
    }
}

/// Convert Trait Implementation for Waker References
impl Into<RawWaker> for WakerRef {
    fn into(self) -> RawWaker {
        let ptr: *const () = self.0.cast().as_ptr() as *const ();
        let waker: RawWaker = RawWaker::new(ptr, &VTABLE);
        // Increment reference count.
        mem::forget(self);
        waker
    }
}

/// Clones the task that is associated to the target [WakerRef].
unsafe fn waker_ref_clone(ptr: *const ()) -> RawWaker {
    let p: WakerRef = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    let q: WakerRef = p.clone();
    // Increment reference count.
    mem::forget(p);
    q.into()
}

/// Wakes up the task that is associated to the target [WakerRef].
unsafe fn waker_ref_wake(ptr: *const ()) {
    let p = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    p.wake();
}

/// Wakes up the task that is associated to the target [WakerRef].
unsafe fn waker_ref_wake_by_ref(ptr: *const ()) {
    let p: WakerRef = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    p.wake_by_ref();
    // Increment reference count.
    mem::forget(p);
}

/// Drops the task that is associated to the target [WakerRef].
unsafe fn waker_ref_drop(ptr: *const ()) {
    let p: WakerRef = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    // Decrement reference count.
    drop(p);
}

/// Raw Waker Trait Implementation for Waker References
pub const VTABLE: RawWakerVTable =
    RawWakerVTable::new(waker_ref_clone, waker_ref_wake, waker_ref_wake_by_ref, waker_ref_drop);

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use crate::scheduler::{
        page::{
            WakerPageRef,
            WakerRef,
        },
        waker64::WAKER_BIT_LENGTH,
    };
    use ::rand::Rng;
    use ::std::ptr::NonNull;
    use ::test::{
        black_box,
        Bencher,
    };

    #[test]
    fn test_refcount() {
        let p: WakerPageRef = WakerPageRef::default();
        assert_eq!(p.refcount_get(), 1);

        let p_clone: NonNull<u8> = p.into_raw_waker_ref(0);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 2);
        let q: WakerRef = WakerRef::new(p_clone);
        let refcount: u64 = q.refcount_get();
        assert_eq!(refcount, 2);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 2);

        let r: WakerRef = WakerRef::new(p.into_raw_waker_ref(31));
        let refcount: u64 = r.refcount_get();
        assert_eq!(refcount, 3);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 3);

        let s: WakerRef = r.clone();
        let refcount: u64 = s.refcount_get();
        assert_eq!(refcount, 4);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 4);

        drop(s);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 3);

        drop(r);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 2);

        drop(q);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 1);
    }

    #[test]
    fn test_wake() {
        let p: WakerPageRef = WakerPageRef::default();
        assert_eq!(p.refcount_get(), 1);

        let q: WakerRef = WakerRef::new(p.into_raw_waker_ref(0));
        assert_eq!(p.refcount_get(), 2);
        let r: WakerRef = WakerRef::new(p.into_raw_waker_ref(31));
        assert_eq!(p.refcount_get(), 3);
        let s: WakerRef = WakerRef::new(p.into_raw_waker_ref(15));
        assert_eq!(p.refcount_get(), 4);

        q.wake();
        assert_eq!(p.take_notified(), 1 << 0);
        assert_eq!(p.refcount_get(), 3);

        r.wake();
        s.wake();
        assert_eq!(p.take_notified(), 1 << 15 | 1 << 31);
        assert_eq!(p.refcount_get(), 1);
    }

    #[test]
    fn test_wake_by_ref() {
        let p: WakerPageRef = WakerPageRef::default();
        assert_eq!(p.refcount_get(), 1);

        let q: WakerRef = WakerRef::new(p.into_raw_waker_ref(0));
        assert_eq!(p.refcount_get(), 2);
        let r: WakerRef = WakerRef::new(p.into_raw_waker_ref(31));
        assert_eq!(p.refcount_get(), 3);
        let s: WakerRef = WakerRef::new(p.into_raw_waker_ref(15));
        assert_eq!(p.refcount_get(), 4);

        q.wake_by_ref();
        assert_eq!(p.take_notified(), 1 << 0);
        assert_eq!(p.refcount_get(), 4);

        r.wake_by_ref();
        s.wake_by_ref();
        assert_eq!(p.take_notified(), 1 << 15 | 1 << 31);
        assert_eq!(p.refcount_get(), 4);

        drop(s);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 3);

        drop(r);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 2);

        drop(q);
        let refcount: u64 = p.refcount_get();
        assert_eq!(refcount, 1);
    }

    #[bench]
    fn bench_wake(b: &mut Bencher) {
        let p: WakerPageRef = WakerPageRef::default();
        let ix: usize = rand::thread_rng().gen_range(0..WAKER_BIT_LENGTH);

        b.iter(|| {
            let raw_page_ref: NonNull<u8> = black_box(p.into_raw_waker_ref(ix));
            let q: WakerRef = WakerRef::new(raw_page_ref);
            q.wake();
        });
    }

    #[bench]
    fn bench_wake_by_ref(b: &mut Bencher) {
        let p: WakerPageRef = WakerPageRef::default();
        let ix: usize = rand::thread_rng().gen_range(0..WAKER_BIT_LENGTH);

        b.iter(|| {
            let raw_page_ref: NonNull<u8> = black_box(p.into_raw_waker_ref(ix));
            let q: WakerRef = WakerRef::new(raw_page_ref);
            q.wake_by_ref();
        });
    }
}
