use std::{
    cell::UnsafeCell,
    mem,
    rc::Rc,
    task::Waker,
};

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
        let s = unsafe {
            let waker = &self.0;
            let cell = &waker.0;
            &mut *cell.get()
        };
        if let Some(ref existing_waker) = s {
            if waker.will_wake(existing_waker) {
                return;
            }
        }
        *s = Some(waker.clone());
    }

    pub fn wake(&self) {
        let s = unsafe {
            let waker = &self.0;
            let cell = &waker.0;
            &mut *cell.get()
        };
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
