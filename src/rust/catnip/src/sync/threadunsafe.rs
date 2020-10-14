#![allow(unused)]
use std::{
    cell::UnsafeCell,
    fmt,
    mem,
    ops::{
        Deref,
        DerefMut,
    },
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

#[derive(Clone)]
pub struct Bytes {
    buf: Option<Rc<[u8]>>,
    offset: usize,
    len: usize,
}

impl PartialEq for Bytes {
    fn eq(&self, rhs: &Self) -> bool {
        &self[..] == &rhs[..]
    }
}

impl Eq for Bytes {}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Bytes({:?})", &self[..])
    }
}

unsafe impl Send for Bytes {}

impl Bytes {
    pub const fn empty() -> Self {
        Self {
            buf: None,
            offset: 0,
            len: 0,
        }
    }

    pub fn split(self, ix: usize) -> (Self, Self) {
        if ix == self.len() {
            return (self, Bytes::empty());
        }
        let buf = self.buf.expect("Can't split an empty buffer");
        assert!(ix < self.len);
        let prefix = Self {
            buf: Some(buf.clone()),
            offset: self.offset,
            len: ix,
        };
        let suffix = Self {
            buf: Some(buf),
            offset: self.offset + ix,
            len: self.len - ix,
        };
        (prefix, suffix)
    }
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self.buf {
            None => &[],
            Some(ref buf) => &buf[self.offset..(self.offset + self.len)],
        }
    }
}

pub struct BytesMut {
    buf: Rc<[u8]>,
}

impl PartialEq for BytesMut {
    fn eq(&self, rhs: &Self) -> bool {
        &self[..] == &rhs[..]
    }
}

impl Eq for BytesMut {}

impl fmt::Debug for BytesMut {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BytesMut({:?})", &self[..])
    }
}

impl From<&[u8]> for BytesMut {
    fn from(buf: &[u8]) -> Self {
        let mut b = Self::zeroed(buf.len());
        b[..].copy_from_slice(buf);
        b
    }
}

impl BytesMut {
    pub fn zeroed(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            buf: unsafe { Rc::new_zeroed_slice(capacity).assume_init() },
        }
    }

    pub fn freeze(self) -> Bytes {
        Bytes {
            offset: 0,
            len: self.buf.len(),
            buf: Some(self.buf),
        }
    }
}

impl Deref for BytesMut {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buf[..]
    }
}

impl DerefMut for BytesMut {
    fn deref_mut(&mut self) -> &mut [u8] {
        Rc::get_mut(&mut self.buf).unwrap()
    }
}
