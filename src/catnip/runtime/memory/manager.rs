// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

pub use super::{
    config::MemoryConfig,
    dpdkbuf::DPDKBuf,
    mbuf::Mbuf,
};
use ::anyhow::Error;
use ::catnip::protocols::{
    ethernet2::ETHERNET2_HEADER_SIZE,
    ipv4::IPV4_HEADER_DEFAULT_SIZE,
    tcp::MAX_TCP_HEADER_SIZE,
};
use ::dpdk_rs::{
    rte_errno,
    rte_mbuf,
    rte_mempool,
    rte_pktmbuf_alloc,
    rte_pktmbuf_clone,
    rte_pktmbuf_free,
    rte_pktmbuf_pool_create,
    rte_socket_id,
    rte_strerror,
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
// Constants
//==============================================================================

const _RTE_PKTMBUF_HEADROOM: usize = 128;

//==============================================================================
// Structures
//==============================================================================

#[derive(Debug)]
pub struct Inner {
    config: MemoryConfig,

    // Used by networking stack for protocol headers + inline bodies. These buffers are only used
    // internally within the network stack.
    header_pool: *mut rte_mempool,

    // Used by networking stack for cloning buffers passed in from the application. There buffers
    // are also only used internally within the networking stack.
    indirect_pool: *mut rte_mempool,

    // Large body pool for buffers given to the application for zero-copy.
    body_pool: *mut rte_mempool,
}

#[derive(Clone, Debug)]
pub struct MemoryManager {
    pub inner: Rc<Inner>,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl MemoryManager {
    pub fn new(max_body_size: usize) -> Result<Self, Error> {
        let memory_config: MemoryConfig =
            MemoryConfig::new(None, None, None, Some(max_body_size), None, None);

        MemoryManager::new_from_config(memory_config)
    }

    fn new_from_config(config: MemoryConfig) -> Result<Self, Error> {
        Ok(Self {
            inner: Rc::new(Inner::new(config)?),
        })
    }

    pub fn make_buffer(&self, packet: *mut rte_mbuf) -> DPDKBuf {
        DPDKBuf::Managed(Mbuf::new(packet, self.clone()))
    }

    pub fn clone_mbuf(&self, mbuf: &Mbuf) -> Mbuf {
        Mbuf::new(self.inner.clone_mbuf(mbuf.get_ptr()), self.clone())
    }

    pub fn into_sgarray(&self, buf: DPDKBuf) -> Result<dmtr_sgarray_t, Fail> {
        let sgaseg = match buf {
            DPDKBuf::External(bytes) => {
                // We have to do a copy here since `Bytes` uses an `Arc<[u8]>` internally and has
                // some additional bookkeeping for an offset and length, but we want to be able to
                // hand off a raw pointer up the application that they can free later.
                let buf_copy: Box<[u8]> = (&bytes[..]).into();
                let ptr = Box::into_raw(buf_copy);
                dmtr_sgaseg_t {
                    sgaseg_buf: ptr as *mut _,
                    sgaseg_len: bytes.len() as u32,
                }
            },
            DPDKBuf::Managed(mbuf) => {
                let sgaseg = dmtr_sgaseg_t {
                    sgaseg_buf: mbuf.data_ptr() as *mut _,
                    sgaseg_len: mbuf.len() as u32,
                };
                mem::forget(mbuf);
                sgaseg
            },
        };
        Ok(dmtr_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    pub fn alloc_header_mbuf(&self) -> Mbuf {
        let mbuf_ptr = unsafe { rte_pktmbuf_alloc(self.inner.header_pool) };
        assert!(!mbuf_ptr.is_null());
        unsafe {
            let num_bytes = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;
            (*mbuf_ptr).data_len = num_bytes as u16;
            (*mbuf_ptr).pkt_len = num_bytes as u32;
        }
        Mbuf::new(mbuf_ptr, self.clone())
    }

    pub fn alloc_body_mbuf(&self) -> Mbuf {
        let mbuf_ptr = unsafe { rte_pktmbuf_alloc(self.inner.body_pool) };
        assert!(!mbuf_ptr.is_null());
        unsafe {
            let num_bytes = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;
            (*mbuf_ptr).data_len = num_bytes as u16;
            (*mbuf_ptr).pkt_len = num_bytes as u32;
        }
        Mbuf::new(mbuf_ptr, self.clone())
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<dmtr_sgarray_t, Fail> {
        // Allocate underlying buffer.
        let (mbuf_ptr, sgaseg): (*mut rte_mbuf, dmtr_sgaseg_t) = if size
            > self.inner.config.get_inline_body_size()
            && size <= self.inner.config.get_max_body_size()
        {
            // Allocate a DPDK-managed buffer.
            let mbuf_ptr: *mut rte_mbuf = unsafe { rte_pktmbuf_alloc(self.inner.body_pool) };
            if mbuf_ptr.is_null() {
                return Err(Fail::new(libc::ENOMEM, "mbuf allocation failed"));
            }

            // TODO: Drop the following warning once DPDK memory management is more stable.
            warn!("allocating mbuf from DPDK pool");

            // Adjust various fields in the mbuf and create a scatter-gather segment out of it.
            unsafe {
                let num_bytes: u16 = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;
                debug_assert!((size as u16) < num_bytes);
                (*mbuf_ptr).data_len = size as u16;
                (*mbuf_ptr).pkt_len = size as u32;
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
            unsafe { rte_pktmbuf_free(mbuf_ptr) };
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
            let body_clone: *mut rte_mbuf = self.inner.clone_mbuf(mbuf_ptr);
            let mut mbuf: Mbuf = Mbuf::new(body_clone, self.clone());
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
        self.inner.body_pool
    }
}

impl Inner {
    fn new(config: MemoryConfig) -> Result<Self, Error> {
        let header_size = ETHERNET2_HEADER_SIZE + IPV4_HEADER_DEFAULT_SIZE + MAX_TCP_HEADER_SIZE;
        let header_mbuf_size = header_size + config.get_inline_body_size();
        let priv_size = 0;
        assert_eq!(
            priv_size, 0,
            "Private data isn't supported (it adds another region between `rte_mbuf` and data)"
        );
        let header_pool = unsafe {
            let name = CString::new("header_pool")?;
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                config.get_header_pool_size() as u32,
                config.get_cache_size() as u32,
                priv_size,
                header_mbuf_size as u16,
                rte_socket_id() as i32,
            )
        };
        if header_pool.is_null() {
            let reason = unsafe { std::ffi::CStr::from_ptr(rte_strerror(rte_errno())) };
            anyhow::bail!("Failed to create header pool: {}", reason.to_str().unwrap())
        }

        let indirect_pool = unsafe {
            let name = CString::new("indirect_pool")?;
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                config.get_indirect_pool_size() as u32,
                config.get_cache_size() as u32,
                priv_size,
                // These mbufs have no body -- they're just for indirect mbufs to point to
                // allocations from the body pool.
                0,
                rte_socket_id() as i32,
            )
        };
        if indirect_pool.is_null() {
            anyhow::bail!("Failed to create indirect pool");
        }

        let body_pool = unsafe {
            let name = CString::new("body_pool")?;
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                config.get_body_pool_size() as u32,
                config.get_cache_size() as u32,
                priv_size,
                config.get_max_body_size() as u16,
                rte_socket_id() as i32,
            )
        };
        if body_pool.is_null() {
            anyhow::bail!("Failed to create body pool");
        }

        Ok(Self {
            config,
            header_pool,
            indirect_pool,
            body_pool,
        })
    }

    pub fn alloc_indirect_empty(&self) -> *mut rte_mbuf {
        let ptr = unsafe { rte_pktmbuf_alloc(self.indirect_pool) };
        assert!(!ptr.is_null());
        ptr
    }

    pub fn clone_mbuf(&self, ptr: *mut rte_mbuf) -> *mut rte_mbuf {
        let ptr = unsafe { rte_pktmbuf_clone(ptr, self.indirect_pool) };
        assert!(!ptr.is_null());
        ptr
    }
}
