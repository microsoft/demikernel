// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::mempool::MemoryPool;
use crate::{
    inetstack::protocols::{
        ethernet2::ETHERNET2_HEADER_SIZE,
        ipv4::IPV4_HEADER_MAX_SIZE,
        tcp::MAX_TCP_HEADER_SIZE,
    },
    runtime::{
        fail::Fail,
        libdpdk::{
            rte_mbuf,
            rte_mempool,
        },
        memory::DemiBuffer,
        types::{
            demi_sgarray_t,
            demi_sgaseg_t,
        },
    },
};
use ::anyhow::Error;
use ::libc::c_void;
use ::std::{
    ffi::CString,
    mem,
    ptr::{
        self,
        NonNull,
    },
    rc::Rc,
};

//==============================================================================
// Exports
//==============================================================================

pub use super::config::MemoryConfig;

//==============================================================================
// Structures
//==============================================================================

// TODO: Drop this structure.
#[derive(Debug)]
pub struct Inner {
    config: MemoryConfig,

    // Used by networking stack for protocol headers + inline bodies. These buffers are only used
    // internally within the network stack.
    header_pool: Rc<MemoryPool>,

    // Large body pool for buffers given to the application for zero-copy.
    body_pool: Rc<MemoryPool>,
}

/// Memory Manager
#[derive(Clone, Debug)]
pub struct MemoryManager {
    inner: Rc<Inner>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associated Functions for Memory Managers
impl MemoryManager {
    /// Instantiates a memory manager.
    pub fn new(max_body_size: usize) -> Result<Self, Error> {
        let memory_config: MemoryConfig = MemoryConfig::new(None, None, Some(max_body_size), None, None);

        Ok(Self {
            inner: Rc::new(Inner::new(memory_config)?),
        })
    }

    /// Converts a runtime buffer into a scatter-gather array.
    pub fn into_sgarray(&self, buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut c_void,
            sgaseg_len: buf.len() as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Allocates a header mbuf.
    /// TODO: Review the need of this function after we are done with the refactor of the DPDK runtime.
    pub fn alloc_header_mbuf(&self) -> Result<DemiBuffer, Fail> {
        let mbuf_ptr: *mut rte_mbuf = self.inner.header_pool.alloc_mbuf(None)?;
        Ok(unsafe { DemiBuffer::from_mbuf(mbuf_ptr) })
    }

    /// Allocates a body mbuf.
    /// TODO: Review the need of this function after we are done with the refactor of the DPDK runtime.
    pub fn alloc_body_mbuf(&self) -> Result<DemiBuffer, Fail> {
        let mbuf_ptr: *mut rte_mbuf = self.inner.body_pool.alloc_mbuf(None)?;
        Ok(unsafe { DemiBuffer::from_mbuf(mbuf_ptr) })
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // ToDo: Allocate an array of buffers if requested size is too large for a single buffer.

        // We can't allocate more than a single buffer.
        if size > u16::MAX as usize {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // First allocate the underlying DemiBuffer.
        let buf: DemiBuffer =
            if size > self.inner.config.get_inline_body_size() && size <= self.inner.config.get_max_body_size() {
                // Allocate a DPDK-managed buffer.
                let mbuf_ptr: *mut rte_mbuf = self.inner.body_pool.alloc_mbuf(Some(size))?;
                // Safety: `mbuf_ptr` is a valid pointer to a properly initialized `rte_mbuf` struct.
                unsafe { DemiBuffer::from_mbuf(mbuf_ptr) }
            } else {
                // Allocate a heap-managed buffer.
                DemiBuffer::new(size as u16)
            };

        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut c_void,
            sgaseg_len: size as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    /// Releases a scatter-gather array.
    pub fn free_sgarray(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid segment count"));
        }

        if sga.sga_buf == ptr::null_mut() {
            return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid DemiBuffer token"));
        }

        // Convert back to a DemiBuffer and drop it.
        // Safety: The `NonNull::new_unchecked()` call is safe, as we verified `sga.sga_buf` is not null above.
        let token: NonNull<u8> = unsafe { NonNull::new_unchecked(sga.sga_buf as *mut u8) };
        // Safety: The `DemiBuffer::from_raw()` call *should* be safe, as the `sga_buf` field in the `demi_sgarray_t`
        // contained a valid `DemiBuffer` token when we provided it to the user (and the user shouldn't change it).
        let buf: DemiBuffer = unsafe { DemiBuffer::from_raw(token) };
        drop(buf);

        Ok(())
    }

    /// Clones a scatter-gather array into a DemiBuffer.
    pub fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
        // Check arguments.
        // TODO: Drop this check once we support scatter-gather arrays with multiple segments.
        if sga.sga_numsegs != 1 {
            return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid segment count"));
        }

        if sga.sga_buf == ptr::null_mut() {
            return Err(Fail::new(libc::EINVAL, "demi_sgarray_t has invalid DemiBuffer token"));
        }

        // Convert back to a DemiBuffer.
        // Safety: The `NonNull::new_unchecked()` call is safe, as we verified `sga.sga_buf` is not null above.
        let token: NonNull<u8> = unsafe { NonNull::new_unchecked(sga.sga_buf as *mut u8) };
        // Safety: The `DemiBuffer::from_raw()` call *should* be safe, as the `sga_buf` field in the `demi_sgarray_t`
        // contained a valid `DemiBuffer` token when we provided it to the user (and the user shouldn't change it).
        let buf: DemiBuffer = unsafe { DemiBuffer::from_raw(token) };
        let mut clone: DemiBuffer = buf.clone();

        // Don't drop buf, as it holds the same reference to the data as the sgarray (which should keep it).
        mem::forget(buf);

        // Check to see if the user has reduced the size of the buffer described by the sgarray segment since we
        // provided it to them.  They could have increased the starting address of the buffer (`sgaseg_buf`),
        // decreased the ending address of the buffer (`sgaseg_buf + sgaseg_len`), or both.
        let sga_data: *const u8 = sga.sga_segs[0].sgaseg_buf as *const u8;
        let sga_len: usize = sga.sga_segs[0].sgaseg_len as usize;
        let clone_data: *const u8 = clone.as_ptr();
        let mut clone_len: usize = clone.len();
        if sga_data != clone_data || sga_len != clone_len {
            // We need to adjust the DemiBuffer to match the user's changes.

            // First check that the user didn't do something non-sensical, like change the buffer description to
            // reference address space outside of the allocated memory area.
            if sga_data < clone_data || sga_data.addr() + sga_len > clone_data.addr() + clone_len {
                return Err(Fail::new(
                    libc::EINVAL,
                    "demi_sgarray_t describes data outside backing buffer's allocated region",
                ));
            }

            // Calculate the amount the new starting address is ahead of the old.  And then adjust `clone` to match.
            let adjustment_amount: usize = sga_data.addr() - clone_data.addr();
            clone.adjust(adjustment_amount)?;

            // An adjustment above would have reduced clone.len() by the adjustment amount.
            clone_len -= adjustment_amount;
            debug_assert_eq!(clone_len, clone.len());

            // Trim the clone down to size.
            let trim_amount: usize = clone_len - sga_len;
            clone.trim(trim_amount)?;
        }

        // Return the clone.
        Ok(clone)
    }

    /// Returns a raw pointer to the underlying body pool.
    /// TODO: Review the need of this function after we are done with the refactor of the DPDK runtime.
    pub fn body_pool(&self) -> *mut rte_mempool {
        self.inner.body_pool.into_raw()
    }
}

/// Associated Functions for Memory Managers
impl Inner {
    fn new(config: MemoryConfig) -> Result<Self, Error> {
        let header_size: usize = ETHERNET2_HEADER_SIZE + (IPV4_HEADER_MAX_SIZE as usize) + MAX_TCP_HEADER_SIZE;
        let header_mbuf_size: usize = header_size + config.get_inline_body_size();

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
