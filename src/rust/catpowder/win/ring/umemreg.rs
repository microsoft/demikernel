// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::{
    mem::MaybeUninit,
    num::{NonZeroU16, NonZeroU32, NonZeroUsize},
    ops::Range,
    ptr::NonNull,
};

use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

use crate::runtime::{
    fail::Fail,
    libxdp,
    memory::{BufferPool, DemiBuffer},
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP user memory region.
pub struct UmemReg {
    _buffer: Vec<MaybeUninit<u8>>,
    pool: BufferPool,
    umem: libxdp::XSK_UMEM_REG,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl UmemReg {
    /// Creates a new XDP user memory region with `count` blocks of `chunk_size` bytes.
    pub fn new(count: NonZeroU32, chunk_size: NonZeroU16) -> Result<Self, Fail> {
        if chunk_size.get() as usize <= BufferPool::overhead_bytes() {
            return Err(Fail::new(libc::EINVAL, "buffer size too small"));
        }

        let buffer_size: u16 = chunk_size.get() - BufferPool::overhead_bytes() as u16;
        let pool: BufferPool = BufferPool::new(buffer_size).map_err(|_| Fail::new(libc::EINVAL, "bad buffer size"))?;
        assert!(pool.pool().layout().size() == chunk_size.get() as usize);

        let total_size: u64 = count.get() as u64 * chunk_size.get() as u64;
        let mut buffer: Vec<MaybeUninit<u8>> = Vec::with_capacity(total_size as usize + pool.pool().layout().align());

        let page_size: NonZeroUsize = get_page_size();
        unsafe { pool.pool().populate(NonNull::from(buffer.as_mut_slice()), page_size)? };

        if pool.pool().is_empty() {
            return Err(Fail::new(libc::ENOMEM, "out of memory"));
        }

        let umem: libxdp::XSK_UMEM_REG = libxdp::XSK_UMEM_REG {
            TotalSize: total_size,
            ChunkSize: chunk_size.get() as u32,
            Headroom: u32::try_from(BufferPool::overhead_bytes()).map_err(Fail::from)?,
            Address: buffer.as_mut_ptr() as *mut core::ffi::c_void,
        };

        Ok(Self {
            _buffer: buffer,
            pool,
            umem,
        })
    }

    /// Get a buffer from the umem pool.
    pub fn get_buffer(&self) -> Option<DemiBuffer> {
        DemiBuffer::new_in_pool(&self.pool)
    }

    /// Gets a reference to the underlying XDP user memory region.
    pub fn as_ref(&self) -> &libxdp::XSK_UMEM_REG {
        &self.umem
    }

    /// Returns a raw pointer to the the start address of the user memory region.
    pub fn address(&self) -> NonNull<u8> {
        // NB: non-nullness is validated by the constructor.
        NonNull::new(self.umem.Address.cast::<u8>()).unwrap()
    }

    /// Returns the region of memory that the umem region occupies.
    pub fn region(&self) -> Range<NonNull<u8>> {
        let start: NonNull<u8> = self.address();
        let end: NonNull<u8> = unsafe { start.add(self.umem.TotalSize as usize) };

        start..end
    }

    /// Get the number of overhead bytes from a DemiBuffer returned by this instance.
    pub fn overhead_bytes(&self) -> usize {
        BufferPool::overhead_bytes()
    }

    /// Determine if the data pointed to by a DemiBuffer is inside the umem region.
    pub fn is_data_in_pool(&self, buf: &DemiBuffer) -> bool {
        let data: NonNull<u8> = unsafe { NonNull::new_unchecked(buf.as_ptr() as *mut u8) };
        self.region().contains(&data)
    }

    /// Dehydrates a DemiBuffer into a usize that can be rehydrated later. This operation consumes the DemiBuffer.
    pub fn dehydrate_buffer(&self, buf: DemiBuffer) -> usize {
        let data: *const u8 = buf.as_ptr();
        let basis: NonNull<u8> = buf.into_raw();

        // Safety: MemoryPool guarantees that the metadata is located immediately prior to the data in a single
        // allocated object.
        if unsafe { data.offset_from(basis.as_ptr()) } != BufferPool::overhead_bytes() as isize {
            panic!("buffer is not properly aligned");
        }

        // Safety: MemoryPool guarantees that the DemiBuffer data is allocated from the allocated object pointed to
        // by `self.address()`.
        unsafe { basis.offset_from(self.address().cast::<u8>()) as usize }
    }

    /// Rehydrates a buffer from an XSK_BUFFER_DESCRIPTOR that was previously dehydrated by `dehydrate_buffer`.
    pub fn rehydrate_buffer_desc(&self, desc: &libxdp::XSK_BUFFER_DESCRIPTOR) -> Result<DemiBuffer, Fail> {
        if desc.Length < BufferPool::overhead_bytes() as u32 {
            return Err(Fail::new(libc::EINVAL, "invalid buffer descriptor"));
        }

        self.rehydrate_buffer_offset(unsafe { desc.Address.__bindgen_anon_1.BaseAddress() })
    }

    /// Rehydrates a buffer from a usize that was previously dehydrated by `dehydrate_buffer`.
    pub fn rehydrate_buffer_offset(&self, offset: u64) -> Result<DemiBuffer, Fail> {
        let token: NonNull<u8> = unsafe { self.address().offset(isize::try_from(offset).map_err(Fail::from)?) };

        Ok(unsafe { DemiBuffer::from_raw(token) })
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================

fn get_page_size() -> NonZeroUsize {
    let mut si: SYSTEM_INFO = SYSTEM_INFO::default();

    // Safety: `si` is allocated and aligned correctly for Windows API access.
    unsafe { GetSystemInfo(&mut si as *mut SYSTEM_INFO) };

    NonZeroUsize::new(si.dwPageSize as usize).expect("invariant violation from Windows API: zero page size")
}
