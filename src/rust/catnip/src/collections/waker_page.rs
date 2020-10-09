use tracy_client::static_span;
use std::{
    alloc::{
        AllocRef,
        Global,
        Layout,
    },
    mem,
    ops::Deref,
    ptr::{
        self,
        NonNull,
    },
    task::{
        RawWaker,
        RawWakerVTable,
    },
};

pub const WAKER_PAGE_SIZE: usize = 64;

mod threadsafe {
    #![allow(unused)]
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use futures::task::AtomicWaker;
    use std::task::Waker;

    pub struct SharedWaker(Arc<AtomicWaker>);

    impl Clone for SharedWaker {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl SharedWaker {
        pub fn new() -> Self {
            Self(Arc::new(AtomicWaker::new()))
        }

        pub fn register(&self, waker: &Waker) {
            self.0.register(waker);
        }

        pub fn wake(&self) {
            self.0.wake();
        }
    }

    pub struct WakerU64(AtomicU64);

    impl WakerU64 {
        pub fn new(val: u64) -> Self {
            WakerU64(AtomicU64::new(val))
        }

        pub fn fetch_or(&self, val: u64) {
            self.0.fetch_or(val, Ordering::SeqCst);
        }

        pub fn fetch_and(&self, val: u64) {
            self.0.fetch_and(val, Ordering::SeqCst);
        }

        pub fn fetch_add(&self, val: u64) -> u64 {
            self.0.fetch_add(val, Ordering::SeqCst)
        }

        pub fn fetch_sub(&self, val: u64) -> u64 {
            self.0.fetch_sub(val, Ordering::SeqCst)
        }

        pub fn load(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }

        pub fn swap(&self, val: u64) -> u64 {
            self.0.swap(val, Ordering::SeqCst)
        }
    }
}
#[cfg(not(feature = "threadunsafe"))]
pub use self::threadsafe::{SharedWaker, WakerU64};

mod threadunsafe {
    #![allow(unused)]
    use std::cell::UnsafeCell;
    use std::mem;
    use std::task::Waker;
    use std::rc::Rc;

    struct WakerSlot(UnsafeCell<Option<Waker>>);

    unsafe impl Send for WakerSlot {}
    unsafe impl Sync for WakerSlot {}

    pub struct SharedWaker(Rc<WakerSlot>);

    impl Clone for SharedWaker {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl SharedWaker {
        pub fn new() -> Self {
            Self(Rc::new(WakerSlot(UnsafeCell::new(None))))
        }

        pub fn register(&self, waker: &Waker) {
            let s = unsafe { &mut *self.0.0.get() };
            if let Some(ref existing_waker) = s {
                if waker.will_wake(existing_waker) {
                    return;
                }
            }
            *s = Some(waker.clone());
        }

        pub fn wake(&self) {
            let s = unsafe { &mut *self.0.0.get() };
            if let Some(waker) = s.take() {
                waker.wake();
            }
        }
    }

    pub struct WakerU64(UnsafeCell<u64>);

    unsafe impl Sync for WakerU64 {}

    impl WakerU64 {
        pub fn new(val: u64) -> Self {
            WakerU64(UnsafeCell::new(val))
        }

        pub fn fetch_or(&self, val: u64) {
            let s = unsafe { &mut *self.0.get() };
            *s |= val;
        }

        pub fn fetch_and(&self, val: u64) {
            let s = unsafe { &mut *self.0.get() };
            *s &= val;
        }

        pub fn fetch_add(&self, val: u64) -> u64 {
            let s = unsafe { &mut *self.0.get() };
            let old = *s;
            *s += val;
            old
        }

        pub fn fetch_sub(&self, val: u64) -> u64 {
            let s = unsafe { &mut *self.0.get() };
            let old = *s;
            *s -= val;
            old
        }

        pub fn load(&self) -> u64 {
            let s = unsafe { &mut *self.0.get() };
            *s
        }

        pub fn swap(&self, val: u64) -> u64 {
            let s = unsafe { &mut *self.0.get() };
            mem::replace(s, val)
        }
    }
}
#[cfg(feature = "threadunsafe")]
pub use self::threadunsafe::{WakerU64, SharedWaker};

#[repr(align(64))]
pub struct WakerPage {
    refcount: WakerU64,
    notified: WakerU64,
    completed: WakerU64,
    dropped: WakerU64,
    waker: SharedWaker,
    _unused: [u8; 24],
}

impl WakerPage {
    pub fn new(waker: SharedWaker) -> WakerPageRef {
        let layout = Layout::new::<WakerPage>();
        assert_eq!(layout.align(), 64);
        let mut ptr: NonNull<WakerPage> = Global.alloc(layout).expect("Allocation failed").cast();
        unsafe {
            let page = ptr.as_mut();
            ptr::write(&mut page.refcount as *mut _, WakerU64::new(1));
            ptr::write(&mut page.notified as *mut _, WakerU64::new(0));
            ptr::write(&mut page.completed as *mut _, WakerU64::new(0));
            ptr::write(&mut page.dropped as *mut _, WakerU64::new(0));
            ptr::write(&mut page.waker as *mut _, waker);
        }
        WakerPageRef(ptr)
    }

    pub fn notify(&self, ix: usize) {
        debug_assert!(ix < 64);
        self.notified.fetch_or(1 << ix);
        self.waker.wake();
    }

    pub fn take_notified(&self) -> u64 {
        // Unset all ready bits, since spurious notifications for completed futures would lead
        // us to poll them after completion.
        let mut notified = self.notified.swap(0);
        notified &= !self.completed.load();
        notified &= !self.dropped.load();
        notified
    }

    pub fn has_completed(&self, ix: usize) -> bool {
        debug_assert!(ix < 64);
        self.completed.load() & (1 << ix) != 0
    }

    pub fn mark_completed(&self, ix: usize) {
        debug_assert!(ix < 64);
        self.completed.fetch_or(1 << ix);
    }

    pub fn mark_dropped(&self, ix: usize) {
        debug_assert!(ix < 64);
        self.dropped.fetch_or(1 << ix);
        self.waker.wake();
    }

    pub fn take_dropped(&self) -> u64 {
        self.dropped.swap(0)
    }

    pub fn was_dropped(&self, ix: usize) -> bool {
        debug_assert!(ix < 64);
        self.dropped.load() & (1 << ix) != 0
    }

    pub fn initialize(&self, ix: usize) {
        debug_assert!(ix < 64);
        self.notified.fetch_or(1 << ix);
        self.completed.fetch_and(!(1 << ix));
        self.dropped.fetch_and(!(1 << ix));
    }

    pub fn clear(&self, ix: usize) {
        debug_assert!(ix < 64);
        let mask = !(1 << ix);
        self.notified.fetch_and(mask);
        self.completed.fetch_and(mask);
        self.dropped.fetch_and(mask);
    }
}

pub struct WakerPageRef(NonNull<WakerPage>);

impl WakerPageRef {
    pub fn raw_waker(&self, ix: usize) -> RawWaker {
        self.waker(ix).into_raw_waker()
    }

    fn waker(&self, ix: usize) -> WakerRef {
        debug_assert!(ix < 64);

        // Bump the refcount for our new reference.
        let self_ = self.clone();
        mem::forget(self_);

        unsafe {
            let base_ptr: *mut u8 = self.0.as_ptr().cast();
            let ptr = NonNull::new_unchecked(base_ptr.offset(ix as isize));
            WakerRef(ptr)
        }
    }

}

impl Clone for WakerPageRef {
    fn clone(&self) -> Self {
        let new_refcount = unsafe {
            // TODO: We could use `Relaxed` here, see `std::sync::Arc` for documentation.
            self.0.as_ref().refcount.fetch_add(1)
        };
        debug_assert!(new_refcount < std::isize::MAX as u64);
        Self(self.0)
    }
}

impl Drop for WakerPageRef {
    fn drop(&mut self) {
        unsafe {
            if self.0.as_ref().refcount.fetch_sub(1) != 1 {
                return;
            }
            ptr::drop_in_place(self.0.as_mut());
            Global.dealloc(self.0.cast(), Layout::for_value(self.0.as_ref()));
        }
    }
}

impl Deref for WakerPageRef {
    type Target = WakerPage;

    fn deref(&self) -> &WakerPage {
        unsafe { self.0.as_ref() }
    }
}

#[repr(transparent)]
struct WakerRef(NonNull<u8>);

impl WakerRef {
    fn base_ptr(&self) -> (NonNull<WakerPage>, usize) {
        let ptr = self.0.as_ptr();

        let forward_offset = ptr.align_offset(64);
        let mut base_ptr = ptr;
        let mut offset = 0;
        if forward_offset != 0 {
            offset = 64 - forward_offset;
            base_ptr = ptr.wrapping_sub(offset);
        }
        unsafe { (NonNull::new_unchecked(base_ptr).cast(), offset) }
    }

    fn wake_by_ref(&self) {
        let (base_ptr, offset) = self.base_ptr();
        let base = unsafe { &*base_ptr.as_ptr() };
        base.notify(offset);
    }

    fn wake(self) {
        self.wake_by_ref()
    }

    fn into_raw_waker(self) -> RawWaker {
        let ptr = self.0.cast().as_ptr() as *const ();
        let waker = RawWaker::new(ptr, &VTABLE);
        mem::forget(self);
        waker
    }
}

unsafe fn waker_ref_clone(ptr: *const ()) -> RawWaker {
    let _s = static_span!();
    let p = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    let q = p.clone();
    mem::forget(p);
    q.into_raw_waker()
}

unsafe fn waker_ref_wake(ptr: *const ()) {
    let _s = static_span!();
    let p = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    p.wake();
}

unsafe fn waker_ref_wake_by_ref(ptr: *const ()) {
    let _s = static_span!();
    let p = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    p.wake_by_ref();
    mem::forget(p);
}

unsafe fn waker_ref_drop(ptr: *const ()) {
    let _s = static_span!();
    let p = WakerRef(NonNull::new_unchecked(ptr as *const u8 as *mut u8));
    drop(p);
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(
    waker_ref_clone,
    waker_ref_wake,
    waker_ref_wake_by_ref,
    waker_ref_drop,
);

impl Clone for WakerRef {
    fn clone(&self) -> Self {
        let (base_ptr, _) = self.base_ptr();
        let p = WakerPageRef(base_ptr);
        mem::forget(p.clone());
        mem::forget(p);
        WakerRef(self.0)
    }
}

impl Drop for WakerRef {
    fn drop(&mut self) {
        let (base_ptr, _) = self.base_ptr();
        // Decrement the refcount.
        drop(WakerPageRef(base_ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::{SharedWaker, WakerPage};
    use std::{
        mem,
    };

    #[test]
    fn test_size() {
        assert_eq!(mem::size_of::<WakerPage>(), 64);
    }

    #[test]
    fn test_basic() {
        let waker = SharedWaker::new();
        let p = WakerPage::new(waker);

        let q = p.waker(0);
        let r = p.waker(63);
        let s = p.waker(16);

        q.wake();
        r.wake();

        assert_eq!(p.take_notified(), 1 << 0 | 1 << 63);

        s.wake();

        assert_eq!(p.take_notified(), 1 << 16);
    }
}
