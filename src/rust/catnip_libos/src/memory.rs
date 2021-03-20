use dpdk_rs::{rte_mbuf, rte_pktmbuf_free};
use std::ops::Deref;
use std::ptr;
use catnip::sync::Bytes;
use catnip::runtime::RuntimeBuf;
use catnip::interop::dmtr_sgarray_t;

#[derive(Debug)]
pub struct Mbuf {
    ptr: *mut rte_mbuf,
}

impl Clone for Mbuf {
    fn clone(&self) -> Self {
        todo!()
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
            DPDKBuf::Managed(ref mbuf) => todo!(),
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
            DPDKBuf::Managed(mbuf) => todo!(),
        }
    }

    fn from_sgarray(sga: &dmtr_sgarray_t) -> Self {
        todo!();
    }
}
