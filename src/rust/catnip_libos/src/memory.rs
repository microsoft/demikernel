use dpdk_rs::{
    rte_mbuf,
    rte_pktmbuf_free,
    rte_pktmbuf_clone,
    rte_pktmbuf_trim,
    rte_pktmbuf_adj,
    rte_pktmbuf_alloc,
    rte_mempool,
};
use std::ops::Deref;
use std::ptr;
use std::slice;
use std::sync::Arc;
use catnip::sync::Bytes;
use catnip::runtime::RuntimeBuf;
use catnip::interop::dmtr_sgarray_t;

const RTE_PKTMBUF_HEADROOM: usize = 128;

#[derive(Clone, Debug)]
pub struct Mempool {
    ptr: Arc<*mut rte_mempool>,
}

impl Mempool {
    fn empty(&self) -> *mut rte_mbuf {
        let ptr = unsafe { rte_pktmbuf_alloc(*self.ptr) };
        assert!(!ptr.is_null());
        ptr
    }
}

#[derive(Debug)]
pub struct Mbuf {
    ptr: *mut rte_mbuf,
    indirect_pool: Mempool,
}

impl Mbuf {
    /// Remove `len` bytes at the beginning of the mbuf.
    pub fn adjust(&mut self, len: usize) {
        assert!(len <= self.len());
        if unsafe { rte_pktmbuf_adj(self.ptr, len as u16) } == ptr::null_mut() {
            panic!("rte_pktmbuf_adj failed");
        }
    }

    /// Remove `len` bytes at the end of the mbuf.
    pub fn trim(&mut self, len: usize) {
        assert!(len <= self.len());
        if unsafe { rte_pktmbuf_trim(self.ptr, len as u16) } != 0 {
            panic!("rte_pktmbuf_trim failed");
        }
    }

    pub fn split(mut self, ix: usize) -> (Self, Self) {
        if ix == self.len() {
            let empty = Self {
                ptr: self.indirect_pool.empty(),
                indirect_pool: self.indirect_pool.clone(),
            };
            return (self, empty);
        }

        let mut suffix = self.clone();
        let mut prefix = self;

        prefix.trim(ix);
        suffix.adjust(ix);
        (prefix, suffix)
    }

    pub fn len(&self) -> usize {
        unsafe { (*self.ptr).data_len as usize - RTE_PKTMBUF_HEADROOM }
    }
}

impl Clone for Mbuf {
    fn clone(&self) -> Self {
        let new_ptr = unsafe {
            rte_pktmbuf_clone(self.ptr, *self.indirect_pool.ptr)
        };
        assert!(!new_ptr.is_null());
        Self { ptr: new_ptr, indirect_pool: self.indirect_pool.clone() }
    }
}

impl Deref for Mbuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        let buf_len = self.len();
        let ptr = unsafe { ((*self.ptr).buf_addr as *mut u8).offset((*self.ptr).data_off as isize) };
        unsafe { slice::from_raw_parts(ptr, buf_len) }
    }
}

impl Drop for Mbuf {
    fn drop(&mut self) {
        if self.ptr.is_null() {
            return;
        }
        unsafe {
            rte_pktmbuf_free(self.ptr);
        }
        self.ptr = ptr::null_mut();
    }
}

#[derive(Clone, Debug)]
pub enum DPDKBuf {
    External(Bytes),
    Managed(Mbuf),
}

impl Deref for DPDKBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            DPDKBuf::External(ref buf) => buf.deref(),
            DPDKBuf::Managed(ref mbuf) => mbuf.deref(),
        }
    }
}

impl RuntimeBuf for DPDKBuf {
    fn empty() -> Self {
        DPDKBuf::External(Bytes::empty())
    }

    fn split(self, ix: usize) -> (Self, Self) {
        match self {
            DPDKBuf::External(buf) => {
                let (prefix, suffix) = buf.split(ix);
                (DPDKBuf::External(prefix), DPDKBuf::External(suffix))
            },
            DPDKBuf::Managed(mbuf) => {
                let (prefix, suffix) = mbuf.split(ix);
                (DPDKBuf::Managed(prefix), DPDKBuf::Managed(suffix))
            },
        }
    }

    fn from_sgarray(sga: &dmtr_sgarray_t) -> Self {
        todo!();
    }
}
