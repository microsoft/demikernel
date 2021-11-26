use anyhow::Error;
use catnip::{
    collections::bytes::{
        Bytes,
        BytesMut,
    },
    interop::{
        dmtr_sgarray_t,
        dmtr_sgaseg_t,
    },
    protocols::{
        ethernet2::frame::ETHERNET2_HEADER_SIZE,
        ipv4::datagram::IPV4_HEADER_SIZE,
        tcp::segment::MAX_TCP_HEADER_SIZE,
    },
    runtime::RuntimeBuf,
};
use dpdk_rs::{
    rte_errno,
    rte_mbuf,
    rte_mempool,
    rte_mempool_calc_obj_size,
    rte_mempool_mem_iter,
    rte_mempool_memhdr,
    rte_mempool_objhdr,
    rte_mempool_objsz,
    rte_pktmbuf_adj,
    rte_pktmbuf_alloc,
    rte_pktmbuf_clone,
    rte_pktmbuf_free,
    rte_pktmbuf_headroom,
    rte_pktmbuf_pool_create,
    rte_pktmbuf_tailroom,
    rte_pktmbuf_trim,
    rte_socket_id,
    rte_strerror,
};
use libc::{
    c_uint,
    c_void,
};
use std::{
    ffi::CString,
    mem,
    ops::Deref,
    ptr,
    rc::Rc,
    slice,
    sync::Arc,
};

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
            header_pool_size: 8191,
            indirect_pool_size: 8191,
            max_body_size: 8320,
            body_pool_size: 8191,
            cache_size: 250,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryManager {
    inner: Rc<Inner>,
}

impl MemoryManager {
    pub fn new(config: MemoryConfig) -> Result<Self, Error> {
        Ok(Self {
            inner: Rc::new(Inner::new(config)?),
        })
    }

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
        let mut mbuf = Mbuf {
            ptr: body_clone,
            mm: self.clone(),
        };

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
        Mbuf {
            ptr: mbuf_ptr,
            mm: self.clone(),
        }
    }

    pub fn alloc_body_mbuf(&self) -> Mbuf {
        let mbuf_ptr = unsafe { rte_pktmbuf_alloc(self.inner.body_pool) };
        assert!(!mbuf_ptr.is_null());
        unsafe {
            let num_bytes = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;
            (*mbuf_ptr).data_len = num_bytes as u16;
            (*mbuf_ptr).pkt_len = num_bytes as u32;
        }
        Mbuf {
            ptr: mbuf_ptr,
            mm: self.clone(),
        }
    }

    pub fn alloc_sgarray(&self, size: usize) -> dmtr_sgarray_t {
        assert!(size <= self.inner.config.max_body_size);

        let sgaseg = if self.inner.config.inline_body_size < size
            && size <= self.inner.config.max_body_size
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
            let mut buf = BytesMut::zeroed(len);
            let seg_slice = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
            buf.copy_from_slice(seg_slice);
            DPDKBuf::External(buf.freeze())
        }
    }

    pub fn body_pool(&self) -> *mut rte_mempool {
        self.inner.body_pool
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
        let priv_size = 0;
        assert_eq!(
            priv_size, 0,
            "Private data isn't supported (it adds another region between `rte_mbuf` and data)"
        );
        let header_pool = unsafe {
            let name = CString::new("header_pool")?;
            rte_pktmbuf_pool_create(
                name.as_ptr(),
                config.header_pool_size as u32,
                config.cache_size as u32,
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
                config.indirect_pool_size as u32,
                config.cache_size as u32,
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
                config.body_pool_size as u32,
                config.cache_size as u32,
                priv_size,
                config.max_body_size as u16,
                rte_socket_id() as i32,
            )
        };
        if body_pool.is_null() {
            anyhow::bail!("Failed to create body pool");
        }

        let mut memory_regions: Vec<(usize, usize)> = vec![];

        extern "C" fn mem_cb(
            mp: *mut rte_mempool,
            opaque: *mut c_void,
            memhdr: *mut rte_mempool_memhdr,
            mem_idx: c_uint,
        ) {
            let mut mr = unsafe { &mut *(opaque as *mut Vec<(usize, usize)>) };
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
        let elt_size = mem::size_of::<rte_mbuf>() + priv_size as usize + config.max_body_size;
        let flags = 0;
        unsafe {
            rte_mempool_calc_obj_size(elt_size as u32, flags, &mut sz as *mut _);
        }
        assert_eq!(sz.header_size, 64);
        assert_eq!(sz.elt_size as usize, 128 + config.max_body_size);
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
        64 + 128 + self.config.max_body_size
    }

    fn alloc_indirect_empty(&self) -> *mut rte_mbuf {
        let ptr = unsafe { rte_pktmbuf_alloc(self.indirect_pool) };
        assert!(!ptr.is_null());
        ptr
    }

    fn clone_mbuf(&self, ptr: *mut rte_mbuf) -> *mut rte_mbuf {
        let ptr = unsafe { rte_pktmbuf_clone(ptr, self.indirect_pool) };
        assert!(!ptr.is_null());
        ptr
    }
}

#[derive(Debug)]
pub struct Mbuf {
    pub ptr: *mut rte_mbuf,
    pub mm: MemoryManager,
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
        let n = self.len();
        if ix == n {
            let empty = Self {
                ptr: self.mm.inner.alloc_indirect_empty(),
                mm: self.mm.clone(),
            };
            return (self, empty);
        }

        let mut suffix = self.clone();
        let mut prefix = self;

        prefix.trim(n - ix);
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
        unsafe { (*self.ptr).data_len as usize }
    }

    pub unsafe fn slice_mut(&mut self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.data_ptr(), self.len())
    }

    pub fn into_raw(mut self) -> *mut rte_mbuf {
        mem::replace(&mut self.ptr, ptr::null_mut())
    }

    pub fn ptr(&mut self) -> *mut rte_mbuf {
        self.ptr
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

    fn from_slice(_: &[u8]) -> Self {
        todo!()
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
}

#[cfg(test)]
mod tests {
    use super::{
        Mbuf,
        MemoryManager,
    };
    use dpdk_rs::*;
    use std::ffi::CString;

    #[test]
    #[ignore]
    fn test_mbuf() {
        //  ["-c", "0xff", "-n", "4", "-w", "37:00.0","--proc-type=auto"]
        let eal_init_args: Vec<CString> = vec![
            CString::new("-c").unwrap(),
            CString::new("0xff").unwrap(),
            CString::new("-n").unwrap(),
            CString::new("4").unwrap(),
            CString::new("-w").unwrap(),
            CString::new("37:00.0").unwrap(),
            CString::new("--proc-type=auto").unwrap(),
        ];
        let eal_init_refs = eal_init_args
            .iter()
            .map(|s| s.as_ptr() as *mut u8)
            .collect::<Vec<_>>();
        unsafe {
            rte_eal_init(eal_init_refs.len() as i32, eal_init_refs.as_ptr() as *mut _);
        }
        let mm = MemoryManager::new(Default::default()).unwrap();

        // Step 1: Allocate a buffer from the body pool.
        let mbuf_ptr = unsafe { rte_pktmbuf_alloc(mm.inner.body_pool) };
        assert!(!mbuf_ptr.is_null());

        // Step 2: Set the length (potentially revealing uninitialized memory)
        // TODO: Provide a "safe" writer interface.
        unsafe {
            let num_bytes = (*mbuf_ptr).buf_len - (*mbuf_ptr).data_off;
            (*mbuf_ptr).data_len = num_bytes as u16;
            (*mbuf_ptr).pkt_len = num_bytes as u32;
        }

        let mut mbuf = Mbuf {
            ptr: mbuf_ptr,
            mm: mm.clone(),
        };

        unsafe {
            let out_slice = std::slice::from_raw_parts_mut(mbuf.data_ptr(), mbuf.len());
            for (i, byte) in out_slice.into_iter().enumerate() {
                *byte = (i % 256) as u8;
            }
        }

        // Consider the mbuf "frozen" at this point. Let's try stripping off some of the "headers"
        // and coming up with a smaller body buffer we'll pass up to the application.
        assert_eq!(mbuf.len(), 2048);
        mbuf.trim(13);
        assert_eq!(mbuf.len(), 2035);
        assert_eq!(mbuf[0], 0);
        mbuf.adjust(22);
        assert_eq!(mbuf.len(), 2013);
        assert_eq!(mbuf[0], 22);
        let (prefix, suffix) = mbuf.split(2012);
        assert_eq!(&suffix[..], &[242]);
        assert_eq!(unsafe { (*prefix.ptr).ol_flags }, 0);
        assert_eq!(unsafe { (*suffix.ptr).ol_flags }, 1 << 62); // IND_ATTACHED_MBUF
        drop(suffix);

        // Let's hold on to this mbuf (as if the application is retaining it) and try to recover
        // its mbuf header from an interior data pointer.
        let data_ptr = unsafe { prefix.data_ptr().offset(10) as *mut libc::c_void };
        let data_len = 17;

        let cloned_mbuf = mm.clone_body(data_ptr, data_len).unwrap();
        assert_eq!(cloned_mbuf[0], 32);
        assert_eq!(cloned_mbuf.len(), 17);
        assert_eq!(unsafe { (*cloned_mbuf.ptr).ol_flags }, 1 << 62);
        drop(cloned_mbuf);
        drop(prefix);
    }
}
