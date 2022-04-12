// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::mempool::MemoryPool;
use ::anyhow::Error;
use ::catnip::protocols::{
    ethernet2::ETHERNET2_HEADER_SIZE,
    ipv4::IPV4_HEADER_DEFAULT_SIZE,
    tcp::MAX_TCP_HEADER_SIZE,
};
use ::dpdk_rs::{
    rte_mbuf,
    rte_mempool,
};
use ::libc::c_void;
use ::runtime::{
    fail::Fail,
    memory::BytesMut,
    types::{
        dmtr_sgarray_t,
        dmtr_sgaseg_t,
    },
};
use ::std::{
    ffi::CString,
    mem,
    ptr,
    rc::Rc,
    slice,
};

//==============================================================================
// Exports
//==============================================================================

pub use super::{
    config::MemoryConfig,
    dpdkbuf::DPDKBuf,
    mbuf::Mbuf,
};

//==============================================================================
// Structures
//==============================================================================

#[derive(Debug)]
pub struct Inner {
    config: MemoryConfig,

    // Used by networking stack for protocol headers + inline bodies. These buffers are only used
    // internally within the network stack.
    header_pool: Rc<MemoryPool>,

    // Large body pool for buffers given to the application for zero-copy.
    body_pool: Rc<MemoryPool>,
}

#[derive(Clone, Debug)]
pub struct MemoryManager {
    inner: Rc<Inner>,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl MemoryManager {
    pub fn new(max_body_size: usize) -> Result<Self, Error> {
        let memory_config: MemoryConfig =
            MemoryConfig::new(None, None, Some(max_body_size), None, None);

        Ok(Self {
            inner: Rc::new(Inner::new(memory_config)?),
        })
    }

    /// Converts a runtime buffer into a scatter-gather array.
    pub fn into_sgarray(&self, buf: DPDKBuf) -> Result<dmtr_sgarray_t, Fail> {
        let (mbuf_ptr, sgaseg): (*mut rte_mbuf, dmtr_sgaseg_t) = match buf {
            // Heap-managed buffer.
            DPDKBuf::External(bytes) => {
                // We have to do a copy here since `Bytes` uses an `Arc<[u8]>` internally and has
                // some additional bookkeeping for an offset and length, but we want to be able to
                // hand off a raw pointer up the application that they can free later.
                let buf_copy: Box<[u8]> = (&bytes[..]).into();
                let ptr: *mut [u8] = Box::into_raw(buf_copy);
                (
                    ptr::null_mut(),
                    dmtr_sgaseg_t {
                        sgaseg_buf: ptr as *mut c_void,
                        sgaseg_len: bytes.len() as u32,
                    },
                )
            },
            // DPDK-managed buffer.
            DPDKBuf::Managed(mbuf) => {
                let mbuf_ptr: *mut rte_mbuf = mbuf.get_ptr();
                let sgaseg: dmtr_sgaseg_t = dmtr_sgaseg_t {
                    sgaseg_buf: mbuf.data_ptr() as *mut c_void,
                    sgaseg_len: mbuf.len() as u32,
                };
                mem::forget(mbuf);
                (mbuf_ptr, sgaseg)
            },
        };

        // TODO: Drop the sga_addr field in the scatter-gather array.
        Ok(dmtr_sgarray_t {
            sga_buf: mbuf_ptr as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Allocates a header mbuf.
    pub fn alloc_header_mbuf(&self) -> Result<Mbuf, Fail> {
        let mbuf_ptr: *mut rte_mbuf = self.inner.header_pool.alloc_mbuf(None)?;
        Ok(Mbuf::new(mbuf_ptr))
    }

    /// Allocates a body mbuf.
    pub fn alloc_body_mbuf(&self) -> Result<Mbuf, Fail> {
        let mbuf_ptr: *mut rte_mbuf = self.inner.body_pool.alloc_mbuf(None)?;
        Ok(Mbuf::new(mbuf_ptr))
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<dmtr_sgarray_t, Fail> {
        // Allocate underlying buffer.
        let (mbuf_ptr, sgaseg): (*mut rte_mbuf, dmtr_sgaseg_t) = if size
            > self.inner.config.get_inline_body_size()
            && size <= self.inner.config.get_max_body_size()
        {
            // Allocate a DPDK-managed buffer.
            let mbuf_ptr: *mut rte_mbuf = self.inner.body_pool.alloc_mbuf(Some(size))?;

            // Adjust various fields in the mbuf and create a scatter-gather segment out of it.
            unsafe {
                let buf_ptr: *mut u8 = (*mbuf_ptr).buf_addr as *mut u8;
                let data_ptr: *mut u8 = buf_ptr.offset((*mbuf_ptr).data_off as isize);
                (
                    mbuf_ptr,
                    dmtr_sgaseg_t {
                        sgaseg_buf: data_ptr as *mut c_void,
                        sgaseg_len: size as u32,
                    },
                )
            }
        } else {
            // Allocate a heap-managed buffer.
            let allocation: Box<[u8]> = unsafe { Box::new_uninit_slice(size).assume_init() };
            let ptr: *mut [u8] = Box::into_raw(allocation);
            (
                ptr::null_mut(),
                dmtr_sgaseg_t {
                    sgaseg_buf: ptr as *mut c_void,
                    sgaseg_len: size as u32,
                },
            )
        };

        // TODO: Drop the sga_addr field in the scatter-gather array.
        Ok(dmtr_sgarray_t {
            sga_buf: mbuf_ptr as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Releases a scatter-gather array.
    pub fn free_sgarray(&self, sga: dmtr_sgarray_t) -> Result<(), Fail> {
        // Bad scatter-gather.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(
                libc::EINVAL,
                "scatter-gather array with invalid size",
            ));
        }

        // Release underlying buffer.
        if !sga.sga_buf.is_null() {
            // Release DPDK-managed buffer.
            let mbuf_ptr: *mut rte_mbuf = sga.sga_buf as *mut rte_mbuf;
            MemoryPool::free_mbuf(mbuf_ptr);
        } else {
            // Release heap-managed buffer.
            let sgaseg: dmtr_sgaseg_t = sga.sga_segs[0];
            let (ptr, len): (*mut c_void, usize) = (sgaseg.sgaseg_buf, sgaseg.sgaseg_len as usize);
            let allocation: Box<[u8]> =
                unsafe { Box::from_raw(slice::from_raw_parts_mut(ptr as *mut _, len)) };
            drop(allocation);
        }

        Ok(())
    }

    /// Clones a scatter-gather array.
    pub fn clone_sgarray(&self, sga: &dmtr_sgarray_t) -> Result<DPDKBuf, Fail> {
        // Bad scatter-gather.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(
                libc::EINVAL,
                "scatter-gather array with invalid size",
            ));
        }

        let sgaseg: dmtr_sgaseg_t = sga.sga_segs[0];
        let (ptr, len): (*mut c_void, usize) = (sgaseg.sgaseg_buf, sgaseg.sgaseg_len as usize);

        // Clone underlying buffer.
        let buf: DPDKBuf = if !sga.sga_buf.is_null() {
            // Clone DPDK-managed buffer.
            let mbuf_ptr: *mut rte_mbuf = sga.sga_buf as *mut rte_mbuf;
            let body_clone: *mut rte_mbuf = match MemoryPool::clone_mbuf(mbuf_ptr) {
                Ok(mbuf_ptr) => mbuf_ptr,
                Err(e) => panic!("failed to clone mbuf: {:?}", e.cause),
            };
            let mut mbuf: Mbuf = Mbuf::new(body_clone);
            // Adjust buffer length.
            // TODO: Replace the following method for computing the length of a mbuf once we have a proper Mbuf abstraction.
            let orig_len: usize = unsafe { ((*mbuf_ptr).buf_len - (*mbuf_ptr).data_off).into() };
            let trim: usize = orig_len - len;
            mbuf.trim(trim);
            DPDKBuf::Managed(mbuf)
        } else {
            // Clone heap-managed buffer.
            let mut buf: BytesMut = BytesMut::zeroed(len).unwrap();
            let seg_slice: &[u8] = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
            buf.copy_from_slice(seg_slice);
            DPDKBuf::External(buf.freeze())
        };

        Ok(buf)
    }

    pub fn body_pool(&self) -> *mut rte_mempool {
        self.inner.body_pool.into_raw()
    }
}

impl Inner {
    fn new(config: MemoryConfig) -> Result<Self, Error> {
        let header_size = ETHERNET2_HEADER_SIZE + IPV4_HEADER_DEFAULT_SIZE + MAX_TCP_HEADER_SIZE;
        let header_mbuf_size = header_size + config.get_inline_body_size();

        // Create memory pool for holding packet headers.
        let header_pool: MemoryPool = MemoryPool::new(
            CString::new("header_pool")?,
            header_mbuf_size,
            config.get_header_pool_size(),
            config.get_cache_size(),
        )?;

        // Create memory pool for holding packet bodies.
        let body_pool: MemoryPool = MemoryPool::new(
            CString::new("body_pool")?,
            config.get_max_body_size(),
            config.get_body_pool_size(),
            config.get_cache_size(),
        )?;

        Ok(Self {
            config,
            header_pool: Rc::new(header_pool),
            body_pool: Rc::new(body_pool),
        })
    }
}
