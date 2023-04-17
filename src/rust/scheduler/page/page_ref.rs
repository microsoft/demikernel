// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::scheduler::{
    page::{
        WakerPage,
        WAKER_PAGE_SIZE,
    },
    waker64::WAKER_BIT_LENGTH,
};
use ::std::{
    alloc::{
        Allocator,
        Global,
        Layout,
    },
    mem,
    ops::Deref,
    ptr::{
        self,
        NonNull,
    },
};

//==============================================================================
// Structures
//==============================================================================

/// Waker Page Reference
///
/// The [crate::Scheduler] relies on this custom reference type to drive the
/// state of futures.
pub struct WakerPageRef(NonNull<WakerPage>);

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Waker Page References
impl WakerPageRef {
    /// Creates a new waker page reference from a non-null reference to a [WakerPage].
    pub fn new(waker_page: NonNull<WakerPage>) -> Self {
        Self(waker_page)
    }

    /// Casts the target [WakerPageRef] into a [NonNull<u8>].
    ///
    /// The reference itself is not intended for reading/writing to
    /// member-fields of a [WakerPage], once it does not point to meaningful
    /// information on this structure. Indeed, the reference is carefully
    /// constructed so that: if points to the base address location of the
    /// structure plus an  offset (in bytes) that identifies some particular
    /// future in the underlying [WakerPage]. The scheduler relies on this hack
    /// to cast back the reference to a [WakerPage] and access the futures
    /// within it correctly.
    ///
    /// This hack only works because: (i) the number of bits in
    /// [crate::waker64::Waker64]s match the number of bytes in [WakerPage]s;
    /// and (ii) [WakerPage]s are aligned to their on memory addresses multiple
    /// of their own size (ie: aligned to N*sizeof([WakerPage])).
    ///
    /// If you don't understand the above explanation, take some time to
    /// carefully review the pointer arithmetic interaction between
    /// [crate::page::WakerPage], [crate::page::WakerPageRef],
    /// [crate::page::WakerRef] and [crate::Scheduler].
    pub fn into_raw_waker_ref(&self, ix: usize) -> NonNull<u8> {
        debug_assert!(ix < WAKER_BIT_LENGTH);

        // Bump the refcount of the underlying waker page.
        let self_: WakerPageRef = self.clone();
        mem::forget(self_);

        unsafe {
            let base_ptr: *mut u8 = self.0.as_ptr().cast();
            let ptr: NonNull<u8> = NonNull::new_unchecked(base_ptr.add(ix));
            ptr
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Clone Trait Implementation for Waker Page References
impl Clone for WakerPageRef {
    fn clone(&self) -> Self {
        let old_refount: u64 = unsafe { self.0.as_ref().refcount_inc() };
        debug_assert!(old_refount < std::u64::MAX);
        Self(self.0)
    }
}

/// Drop Trait Implementation for Waker Page References
impl Drop for WakerPageRef {
    fn drop(&mut self) {
        match unsafe { self.0.as_ref().refcount_dec() } {
            Some(1) => unsafe {
                ptr::drop_in_place(self.0.as_mut());
                Global.deallocate(self.0.cast(), Layout::for_value(self.0.as_ref()));
            },
            Some(_) => {},
            None => panic!("double free on waker page {:?}", self.0),
        }
    }
}

/// De-Reference Trait Implementation for Waker Page References
impl Deref for WakerPageRef {
    type Target = WakerPage;

    fn deref(&self) -> &WakerPage {
        unsafe { self.0.as_ref() }
    }
}

/// Default Trait Implementation for Waker Page References
impl Default for WakerPageRef {
    fn default() -> Self {
        let layout: Layout = Layout::new::<WakerPage>();
        assert_eq!(layout.align(), WAKER_PAGE_SIZE);
        let mut ptr: NonNull<WakerPage> = Global.allocate(layout).expect("Failed to allocate WakerPage").cast();
        unsafe {
            let page: &mut WakerPage = ptr.as_mut();
            page.reset();
        }
        Self(ptr)
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use crate::scheduler::page::WakerPageRef;
    use ::anyhow::Result;
    use ::std::ptr::NonNull;

    #[test]
    fn test_clone() -> Result<()> {
        let p: WakerPageRef = WakerPageRef::default();

        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 1);

        // Clone p
        let p_clone: WakerPageRef = p.clone();
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 2);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 2);

        // Clone p_clone
        let p_clone_clone: WakerPageRef = p_clone.clone();
        let refcount: u64 = p_clone_clone.refcount_get();
        crate::ensure_eq!(refcount, 3);
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 3);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 3);

        // Drop p_clone_clone
        drop(p_clone_clone);
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 2);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 2);

        // Drop p_clone
        drop(p_clone);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 1);

        // Clone p
        let p_clone: WakerPageRef = p.clone();
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 2);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 2);

        // Clone p again
        let p_clone_clone: WakerPageRef = p.clone();
        let refcount: u64 = p_clone_clone.refcount_get();
        crate::ensure_eq!(refcount, 3);
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 3);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 3);

        // Drop p
        drop(p);
        let refcount: u64 = p_clone_clone.refcount_get();
        crate::ensure_eq!(refcount, 2);
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 2);

        // Drop p_clone_clone
        drop(p_clone_clone);
        let refcount: u64 = p_clone.refcount_get();
        crate::ensure_eq!(refcount, 1);

        Ok(())
    }

    #[test]
    fn test_into() -> Result<()> {
        let p: WakerPageRef = WakerPageRef::default();

        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 1);

        let _: NonNull<u8> = p.into_raw_waker_ref(0);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 2);

        let _: NonNull<u8> = p.into_raw_waker_ref(31);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 3);

        let _: NonNull<u8> = p.into_raw_waker_ref(63);
        let refcount: u64 = p.refcount_get();
        crate::ensure_eq!(refcount, 4);

        Ok(())
    }
}
