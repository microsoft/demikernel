use std::{
    fmt,
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
};
use crate::runtime::RuntimeBuf;

mod threadunsafe;
mod threadsafe;

pub use self::threadunsafe::{SharedWaker, WakerU64};

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
