use std::ffi::CString;
use dpdk_rs::{
    rte_mbuf,
    rte_pktmbuf_pool_create,
    rte_pktmbuf_free,
    rte_pktmbuf_clone,
    rte_pktmbuf_trim,
    rte_pktmbuf_adj,
    rte_pktmbuf_alloc,
    rte_mempool,
    rte_mempool_objhdr,
    rte_mempool_memhdr,
    rte_socket_id,
    rte_mempool_mem_iter,
    rte_pktmbuf_headroom,
    rte_pktmbuf_tailroom,
};
use catnip::protocols::tcp::segment::MAX_TCP_HEADER_SIZE;
use catnip::protocols::ipv4::datagram::IPV4_HEADER_SIZE;
use catnip::protocols::ethernet2::frame::ETHERNET2_HEADER_SIZE;
use std::ops::Deref;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::Arc;
use std::rc::Rc;
use catnip::sync::Bytes;
use catnip::runtime::RuntimeBuf;
use catnip::interop::dmtr_sgarray_t;
use libc::{c_uint, c_void};
use anyhow::Error;

const RTE_PKTMBUF_HEADROOM: usize = 128;

#[derive(Clone, Copy, Debug)]
pub struct MemoryConfig {
    /// What is the cutoff point for copying application buffers into reserved body space within a
    /// header `mbuf`? Smaller values copy less but incur the fixed cost of chaining together
    /// `mbuf`s earlier.
    pub inline_body_size: usize,

    /// How many buffers are within the header pool?
    pub header_pool_size: usize,

    /// How many buffers are within the indirect pool?
    pub indirect_pool_size: usize,

    /// What is the maximum body size? This should effectively be the MSS + RTE_PKTMBUF_HEADROOM.
    pub max_body_size: usize,

    /// How many buffers are within the body pool?
    pub body_pool_size: usize,

    /// How many buffers should remain within `rte_mempool`'s per-thread cache?
    pub cache_size: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            inline_body_size: 1024,
            header_pool_size: 256,
            indirect_pool_size: 256,
            max_body_size: 2176,
            body_pool_size: 8192,
            cache_size: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryManager {
    inner: Rc<Inner>,
}

impl MemoryManager {
    fn clone_mbuf(&self, mbuf: &Mbuf) -> Mbuf {
        Mbuf {
            ptr: self.inner.clone_mbuf(mbuf.ptr),
            mm: self.clone(),
        }
    }

    /// Given a pointer and length into a body `mbuf`, return a fresh indirect `mbuf` that points to
    /// the same memory region, incrementing the refcount of the body `mbuf`.
    pub fn clone_body(&self, ptr: *mut c_void, len: usize) -> Result<Mbuf, Error> {
        let mbuf_ptr = self.recover_body_mbuf(ptr)?;
        let body_clone = self.inner.clone_mbuf(mbuf_ptr);

        // Wrap the mbuf first so we free it on early exit.
        let mut mbuf = Mbuf { ptr: body_clone, mm: self.clone() };

        let orig_ptr = mbuf.data_ptr();
        let orig_len = mbuf.len();

        if (ptr as usize) < (orig_ptr as usize) {
            anyhow::bail!("Trying to recover data pointer outside original body: {:?} vs. {:?}", ptr, orig_ptr);
        }
        let adjust = ptr as usize - orig_ptr as usize;

        if adjust + len > orig_len {
            anyhow::bail!("Recovering too many bytes: {} + {} > {}", adjust, len, orig_len);
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
            anyhow::bail!("Data pointer within allocation header: {:?} in {:?}", ptr, self.inner);
        }

        let mbuf_ptr = (ptr_int - offset_within_alloc + 64) as *mut rte_mbuf;
        Ok(mbuf_ptr)
    }

    fn is_body_ptr(&self, ptr: *mut c_void) -> bool {
        let ptr_int = ptr as usize;
        let body_end = self.inner.body_region_addr + self.inner.body_region_len;
        ptr_int >= self.inner.body_region_addr && ptr_int < body_end
    }
}

#[derive(Debug)]
struct Inner {
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

impl Inner {
    fn new(config: MemoryConfig) -> Result<Self, Error> {
        let header_size = ETHERNET2_HEADER_SIZE + IPV4_HEADER_SIZE + MAX_TCP_HEADER_SIZE;
        let header_mbuf_size = header_size + config.inline_body_size;
        let header_pool = unsafe {
            let name = CString::new("header_pool")?;
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                config.header_pool_size as u32,
                config.cache_size as u32,
                0,
                header_mbuf_size as u16,
                rte_socket_id() as i32,
            )
        };
        if header_pool.is_null() {
            anyhow::bail!("Failed to create header pool");
        }

        let indirect_pool = unsafe {
            let name = CString::new("indirect_pool")?;
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                config.indirect_pool_size as u32,
                config.cache_size as u32,
                0,
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
                config.body_pool_size as u32,
                config.cache_size as u32,
                0,
                config.max_body_size as u16,
                rte_socket_id() as i32,
            )
        };
        if body_pool.is_null() {
            anyhow::bail!("Failed to create body pool");
        }

        let mut memory_regions: Vec<(usize, usize)> = vec![];

        extern "C" fn mem_cb(mp: *mut rte_mempool, opaque: *mut c_void, memhdr: *mut rte_mempool_memhdr, mem_idx: c_uint) {
            println!("Memchunk {}: {:?}", mem_idx, unsafe {*memhdr});
            let mut mr = unsafe { &mut *(opaque as *mut Vec<(usize, usize)>) };
            let (addr, len) = unsafe { ((*memhdr).addr, (*memhdr).len) };
            mr.push((addr as usize, len as usize));
        }
        unsafe {
            rte_mempool_mem_iter(body_pool, Some(mem_cb), &mut memory_regions as *mut _ as *mut c_void);
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
        assert_eq!(mem::size_of::<rte_mempool_objhdr>(), 64);
        assert_eq!(mem::size_of::<rte_mbuf>(), 128);
        64 + 128 + self.config.max_body_size
    }

    fn alloc_indirect_empty(&self) -> *mut rte_mbuf {
        let ptr = unsafe { rte_pktmbuf_alloc(self.indirect_pool) };
        assert!(!ptr.is_null());
        ptr
    }

    fn clone_mbuf(&self, ptr: *mut rte_mbuf) -> *mut rte_mbuf {
        let ptr = unsafe {
            rte_pktmbuf_clone(ptr, self.indirect_pool)
        };
        assert!(!ptr.is_null());
        ptr
    }
}

#[derive(Debug)]
pub struct Mbuf {
    ptr: *mut rte_mbuf,
    mm: MemoryManager,
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
                ptr: self.mm.inner.alloc_indirect_empty(),
                mm: self.mm.clone(),
            };
            return (self, empty);
        }

        let mut suffix = self.clone();
        let mut prefix = self;

        prefix.trim(ix);
        suffix.adjust(ix);
        (prefix, suffix)
    }

    pub fn data_ptr(&self) -> *mut u8 {
        unsafe {
            let buf_ptr = (*self.ptr).buf_addr as *mut u8;
            buf_ptr.offset((*self.ptr).data_off as isize)
        }
    }

    pub fn len(&self) -> usize {
        unsafe {
            let headroom = rte_pktmbuf_headroom(self.ptr) as usize;
            let tailroom = rte_pktmbuf_tailroom(self.ptr) as usize;
            (*self.ptr).data_len as usize - headroom - tailroom
        }
    }
}

impl Clone for Mbuf {
    fn clone(&self) -> Self {
        self.mm.clone_mbuf(self)
    }
}

impl Deref for Mbuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data_ptr(), self.len()) }
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

    fn adjust(&mut self, num_bytes: usize) {
        match self {
            DPDKBuf::External(ref mut buf) => buf.adjust(num_bytes),
            DPDKBuf::Managed(ref mut mbuf) => mbuf.adjust(num_bytes),
        }
    }

    fn trim(&mut self, num_bytes: usize) {
        match self {
            DPDKBuf::External(ref mut buf) => buf.trim(num_bytes),
            DPDKBuf::Managed(ref mut mbuf) => mbuf.trim(num_bytes),
        }
    }

    fn from_sgarray(sga: &dmtr_sgarray_t) -> Self {
        // TODO: Scatter/gather support.
        assert_eq!(sga.sga_numsegs, 1);
        DPDKBuf::External(Bytes::from_sgarray(sga))
    }
}

#[cfg(test)]
mod tests {
    use super::Mbuf;

    #[test]
    fn test_mbuf() {
        todo!()
    }
}
