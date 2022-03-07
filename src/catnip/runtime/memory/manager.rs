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
    ipv4::IPV4_HEADER_SIZE,
    tcp::MAX_TCP_HEADER_SIZE,
};
use ::dpdk_rs::{
    rte_errno,
    rte_mbuf,
    rte_mempool,
    rte_mempool_calc_obj_size,
    rte_mempool_mem_iter,
    rte_mempool_memhdr,
    rte_mempool_objsz,
    rte_pktmbuf_alloc,
    rte_pktmbuf_clone,
    rte_pktmbuf_free,
    rte_pktmbuf_pool_create,
    rte_socket_id,
    rte_strerror,
};
use ::libc::{
    c_uint,
    c_void,
};
use ::runtime::{
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

    // We assert that the body pool's memory region is in a single, contiguous virtual memory
    // region. Here is a diagram of the memory layout of `body_pool`.
    //
    //     |- rte_mempool_objhdr (64 bytes) -|- rte_mbuf (128 bytes) -|- data -| - rte_mempool_objhdr - |- ...
    //     ^                                                                   ^
    //     body_region_addr                                                    body_region_addr + Self::body_alloc_size()
    //
    // This way, we can easily test to see if an arbitrary pointer is within our `body_pool` via
    // pointer arithmetic. Then, by understanding the layout of each allocation, we can take a
    // pointer to somewhere *within* an allocation and work backwards to a pointer to its
    // `rte_mbuf`, which we can then clone for use within our network stack.
    //
    body_region_addr: usize,
    body_region_len: usize,
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

    /// Given a pointer and length into a body `mbuf`, return a fresh indirect `mbuf` that points to
    /// the same memory region, incrementing the refcount of the body `mbuf`.
    pub fn clone_body(&self, ptr: *mut c_void, len: usize) -> Result<Mbuf, Error> {
        let mbuf_ptr = self.recover_body_mbuf(ptr)?;
        let body_clone = self.inner.clone_mbuf(mbuf_ptr);

        // Wrap the mbuf first so we free it on early exit.
        let mut mbuf = Mbuf::new(body_clone, self.clone());

        let orig_ptr = mbuf.data_ptr();
        let orig_len = mbuf.len();

        if (ptr as usize) < (orig_ptr as usize) {
            anyhow::bail!(
                "Trying to recover data pointer outside original body: {:?} vs. {:?}",
                ptr,
                orig_ptr
            );
        }
        let adjust = ptr as usize - orig_ptr as usize;

        if adjust + len > orig_len {
            anyhow::bail!(
                "Recovering too many bytes: {} + {} > {}",
                adjust,
                len,
                orig_len
            );
        }
        let trim = orig_len - (adjust + len);

        mbuf.adjust(adjust);
        mbuf.trim(trim);

        Ok(mbuf)
    }

    fn recover_body_mbuf(&self, ptr: *mut c_void) -> Result<*mut rte_mbuf, Error> {
        if !self.is_body_ptr(ptr) {
            anyhow::bail!("Out of bounds ptr {:?}", ptr);
        }

        let ptr_int = ptr as usize;
        let ptr_offset = ptr_int - self.inner.body_region_addr;
        let offset_within_alloc = ptr_offset % self.inner.body_alloc_size();

        if offset_within_alloc < (64 + 128) {
            anyhow::bail!(
                "Data pointer within allocation header: {:?} in {:?}",
                ptr,
                self.inner
            );
        }

        let mbuf_ptr = (ptr_int - offset_within_alloc + 64) as *mut rte_mbuf;
        Ok(mbuf_ptr)
    }

    fn is_body_ptr(&self, ptr: *mut c_void) -> bool {
        let ptr_int = ptr as usize;
        let body_end = self.inner.body_region_addr + self.inner.body_region_len;
        ptr_int >= self.inner.body_region_addr && ptr_int < body_end
    }

    pub fn into_sgarray(&self, buf: DPDKBuf) -> dmtr_sgarray_t {
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
        dmtr_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        }
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

    pub fn alloc_sgarray(&self, size: usize) -> dmtr_sgarray_t {
        assert!(size <= self.inner.config.get_max_body_size());

        let sgaseg = if self.inner.config.get_inline_body_size() < size
            && size <= self.inner.config.get_max_body_size()
        {
            let mbuf_ptr = unsafe { rte_pktmbuf_alloc(self.inner.body_pool) };
            assert!(!mbuf_ptr.is_null());
            unsafe {
                let num_bytes = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;
                // We don't strictly have to set these fields, since we don't directly hand off body
                // `mbuf`s to `rte_eth_tx_burst`, but it's nice to have the original allocation size around.
                assert!(size as u16 <= num_bytes);
                (*mbuf_ptr).data_len = size as u16;
                (*mbuf_ptr).pkt_len = size as u32;
                let buf_ptr = (*mbuf_ptr).buf_addr as *mut u8;
                let data_ptr = buf_ptr.offset((*mbuf_ptr).data_off as isize);
                dmtr_sgaseg_t {
                    sgaseg_buf: data_ptr as *mut _,
                    sgaseg_len: size as u32,
                }
            }
        } else {
            let allocation: Box<[u8]> = unsafe { Box::new_uninit_slice(size).assume_init() };
            let ptr = Box::into_raw(allocation);
            dmtr_sgaseg_t {
                sgaseg_buf: ptr as *mut _,
                sgaseg_len: size as u32,
            }
        };
        dmtr_sgarray_t {
            sga_buf: ptr::null_mut(),
            sga_numsegs: 1,
            sga_segs: [sgaseg],
            sga_addr: unsafe { mem::zeroed() },
        }
    }

    pub fn free_sgarray(&self, sga: dmtr_sgarray_t) {
        assert_eq!(sga.sga_numsegs, 1);
        let sgaseg = sga.sga_segs[0];
        let (ptr, len) = (sgaseg.sgaseg_buf, sgaseg.sgaseg_len as usize);

        if self.is_body_ptr(ptr) {
            let mbuf_ptr = self.recover_body_mbuf(ptr).expect("Invalid sga pointer");
            unsafe { rte_pktmbuf_free(mbuf_ptr) };
        } else {
            let allocation: Box<[u8]> =
                unsafe { Box::from_raw(slice::from_raw_parts_mut(ptr as *mut _, len)) };
            drop(allocation);
        }
    }

    pub fn clone_sgarray(&self, sga: &dmtr_sgarray_t) -> DPDKBuf {
        assert_eq!(sga.sga_numsegs, 1);
        let sgaseg = sga.sga_segs[0];
        let (ptr, len) = (sgaseg.sgaseg_buf, sgaseg.sgaseg_len as usize);

        if self.is_body_ptr(ptr) {
            let mbuf = self.clone_body(ptr, len).expect("Invalid sga pointer");
            DPDKBuf::Managed(mbuf)
        } else {
            let mut buf = BytesMut::zeroed(len).unwrap();
            let seg_slice = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
            buf.copy_from_slice(seg_slice);
            DPDKBuf::External(buf.freeze())
        }
    }

    pub fn body_pool(&self) -> *mut rte_mempool {
        self.inner.body_pool
    }
}

impl Inner {
    fn new(config: MemoryConfig) -> Result<Self, Error> {
        let header_size = ETHERNET2_HEADER_SIZE + IPV4_HEADER_SIZE + MAX_TCP_HEADER_SIZE;
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

        let mut memory_regions: Vec<(usize, usize)> = vec![];

        extern "C" fn mem_cb(
            _mp: *mut rte_mempool,
            opaque: *mut c_void,
            memhdr: *mut rte_mempool_memhdr,
            _mem_idx: c_uint,
        ) {
            let mr = unsafe { &mut *(opaque as *mut Vec<(usize, usize)>) };
            let (addr, len) = unsafe { ((*memhdr).addr, (*memhdr).len) };
            mr.push((addr as usize, len as usize));
        }
        unsafe {
            rte_mempool_mem_iter(
                body_pool,
                Some(mem_cb),
                &mut memory_regions as *mut _ as *mut c_void,
            );
        }
        memory_regions.sort();

        if memory_regions.is_empty() {
            anyhow::bail!("Allocated body pool without any memory regions?");
        }

        let (base_addr, base_len) = memory_regions[0];
        let mut cur_addr = base_addr + base_len;

        for &(addr, len) in &memory_regions[1..] {
            if addr != cur_addr {
                anyhow::bail!("Non-contiguous memory regions {} vs. {}", cur_addr, addr);
            }
            cur_addr += len;
        }
        let total_len = cur_addr - base_addr;

        // Check our assumptions in `body_alloc_size` for how each object in the mempool is laid
        // out in the mempool ring.
        let mut sz: rte_mempool_objsz = unsafe { mem::zeroed() };
        let elt_size = mem::size_of::<rte_mbuf>() + priv_size as usize + config.get_max_body_size();
        let flags = 0;
        unsafe {
            rte_mempool_calc_obj_size(elt_size as u32, flags, &mut sz as *mut _);
        }
        assert_eq!(sz.header_size, 64);
        assert_eq!(sz.elt_size as usize, 128 + config.get_max_body_size());
        assert_eq!(sz.trailer_size, 0);

        Ok(Self {
            config,
            header_pool,
            indirect_pool,
            body_pool,

            body_region_addr: base_addr,
            body_region_len: total_len,
        })
    }

    fn body_alloc_size(&self) -> usize {
        64 + 128 + self.config.get_max_body_size()
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
