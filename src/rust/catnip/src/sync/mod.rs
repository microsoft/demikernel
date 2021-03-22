use futures::task::AtomicWaker;
use std::{
    fmt,
    ops::{
        Deref,
        DerefMut,
    },
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
    },
    task::Waker,
};
use crate::runtime::RuntimeBuf;

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

#[derive(Clone)]
pub struct Bytes {
    buf: Option<Arc<[u8]>>,
    offset: usize,
    len: usize,
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Bytes({:?})", &self[..])
    }
}

impl PartialEq for Bytes {
    fn eq(&self, rhs: &Self) -> bool {
        &self[..] == &rhs[..]
    }
}

impl Eq for Bytes {}

impl RuntimeBuf for Bytes {
    fn empty() -> Self {
        Self {
            buf: None,
            offset: 0,
            len: 0,
        }
    }

    fn adjust(&mut self, num_bytes: usize) {
        if num_bytes > self.len {
            panic!("Adjusting past end of buffer: {} vs. {}", num_bytes, self.len);
        }
        self.offset += num_bytes;
        self.len -= num_bytes;
    }

    fn trim(&mut self, num_bytes: usize) {
        if num_bytes > self.len {
            panic!("Trimming past beginning of buffer: {} vs. {}", num_bytes, self.len);
        }
        self.len -= num_bytes;
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
    buf: Arc<[u8]>,
}

impl fmt::Debug for BytesMut {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BytesMut({:?})", &self[..])
    }
}

impl PartialEq for BytesMut {
    fn eq(&self, rhs: &Self) -> bool {
        &self[..] == &rhs[..]
    }
}

impl Eq for BytesMut {}

impl BytesMut {
    pub fn zeroed(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            buf: unsafe { Arc::new_zeroed_slice(capacity).assume_init() },
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

impl From<&[u8]> for BytesMut {
    fn from(buf: &[u8]) -> Self {
        let mut b = Self::zeroed(buf.len());
        b[..].copy_from_slice(buf);
        b
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
        Arc::get_mut(&mut self.buf).unwrap()
    }
}
