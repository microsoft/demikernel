// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::{
    mem::MaybeUninit,
    num::{NonZeroU16, NonZeroU32, NonZeroUsize},
    ops::Range,
    ptr::NonNull,
};

use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

use crate::{
    catpowder::win::{api::XdpApi, socket::XdpSocket},
    runtime::{
        fail::Fail,
        libxdp,
        memory::{BufferPool, DemiBuffer},
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// A wrapper structure for a XDP user memory region.
pub struct UmemReg {
    _buffer: Vec<MaybeUninit<u8>>,
    pool: BufferPool,
    reserve_pool: Option<BufferPool>,
    umem: libxdp::XSK_UMEM_REG,
    buf_offset_from_chunk: isize,
}

//======================================================================================================================
// Implementations
//======================================================================================================================

impl UmemReg {
    /// Creates a new XDP user memory region with `count` blocks of `chunk_size` bytes.
    pub fn new(
        api: &mut XdpApi,
        socket: &mut XdpSocket,
        count: NonZeroU32,
        chunk_size: NonZeroU16,
        reserve_count: u32,
    ) -> Result<Self, Fail> {
        let pool: BufferPool =
            BufferPool::new(chunk_size.get()).map_err(|_| Fail::new(libc::EINVAL, "bad buffer size"))?;
        assert!(pool.pool().layout().size() >= chunk_size.get() as usize);

        let page_size: NonZeroUsize = get_page_size();
        let real_chunk_size: usize = pool.pool().layout().size();

        // NB when the page size is not evenly divisible by the number of bytes in the last page
        // of a buffer, BufferPool will pad the buffer to prevent excess page spanning; however,
        // XDP requires that buffers are contiguous without padding. So here we grow the buffer to
        // ensure we are not padding.
        let real_last_page_bytes: usize = real_chunk_size % page_size.get();
        let (pool, chunk_size): (BufferPool, u16) = if real_chunk_size % pool.pool().layout().align() != 0
            || (real_last_page_bytes > 0 && page_size.get() % real_last_page_bytes != 0)
        {
            // Take the following example:
            // Chunk size = 1500 (standard MTU), page size = 4096 (standard page size).
            // DemiBuffers add 128 bytes of overhead and align to 64 byte boundaries (cache line)
            // real_chunk_size = 1628
            // aligned_chunk_size = 1664 (aligned to 64 bytes)
            // Two buffers span 3328 bytes, which leaves 768 bytes left in the page. Since
            // BufferPool tries not span pages, we have to assign these 768 bytes into the two
            // buffers, giving us 2048 bytes per buffer.
            // This means we have to grow our original chunk size by 2048 - 1628 = 420 bytes.
            let headroom_bytes: usize = real_chunk_size - chunk_size.get() as usize;
            let old_last_page_bytes: usize = chunk_size.get() as usize % page_size.get();

            let aligned_real_chunk_size: usize =
                real_chunk_size + (pool.pool().layout().align() - (real_chunk_size % pool.pool().layout().align()));
            let aligned_real_last_page_bytes: usize = aligned_real_chunk_size % page_size.get();
            let aligned_real_last_page_bytes: usize = aligned_real_last_page_bytes.next_power_of_two();

            let chunk_diff: usize = aligned_real_last_page_bytes - headroom_bytes - old_last_page_bytes;
            let new_chunk_size: u16 = chunk_size.get() + chunk_diff as u16;

            trace!("growing chunk size from {} to {}", chunk_size.get(), new_chunk_size);

            let pool: BufferPool =
                BufferPool::new(new_chunk_size).map_err(|_| Fail::new(libc::EINVAL, "bad buffer size"))?;
            (pool, new_chunk_size)
        } else {
            (pool, chunk_size.get())
        };

        let reserve_pool: Option<BufferPool> = if reserve_count > 0 {
            let reserve_pool: BufferPool =
                BufferPool::new(chunk_size).map_err(|_| Fail::new(libc::EINVAL, "bad buffer size"))?;
            assert!(reserve_pool.pool().layout() == pool.pool().layout());
            Some(reserve_pool)
        } else {
            None
        };

        let real_chunk_size: usize = pool.pool().layout().size();
        let headroom: usize = real_chunk_size - chunk_size as usize;
        let buf_offset_from_chunk: isize =
            isize::try_from(headroom - BufferPool::overhead_bytes()).map_err(Fail::from)?;

        let align: usize = std::cmp::max(page_size.get(), pool.pool().layout().align());
        let total_size: u64 = (count.get() as u64 + reserve_count as u64) * real_chunk_size as u64 + align as u64;

        trace!(
            "creating umem region with {} blocks of {} bytes aligned to {} with headroom {} and DemiBuffer offset of {}",
            count.get() + reserve_count,
            real_chunk_size,
            align,
            headroom,
            buf_offset_from_chunk
        );
        let mut buffer: Vec<MaybeUninit<u8>> = Vec::new();
        buffer.resize(total_size as usize, MaybeUninit::uninit());

        let offset: usize = buffer.as_mut_ptr().align_offset(align);
        let total_size: u64 = total_size - offset as u64;

        // Round down to the nearest multiple of the real chunk size.
        let total_size: u64 = total_size - (total_size % real_chunk_size as u64);

        let main_pool_size = count.get() as u64 * real_chunk_size as u64;
        let buffer_ptr: NonNull<[MaybeUninit<u8>]> =
            NonNull::from(&mut buffer[offset..(offset + main_pool_size as usize)]);
        unsafe { pool.pool().populate(buffer_ptr, page_size)? };

        debug!("populated umem pool with {} buffers", pool.pool().len());
        if pool.pool().is_empty() {
            return Err(Fail::new(libc::ENOMEM, "out of memory"));
        }

        if let Some(reserve_pool) = reserve_pool.as_ref() {
            let reserve_ptr: NonNull<[MaybeUninit<u8>]> =
                NonNull::from(&mut buffer[(offset + main_pool_size as usize)..(offset + total_size as usize)]);
            unsafe { reserve_pool.pool().populate(reserve_ptr, page_size)? };

            debug!("populated umem reserve pool with {} buffers", reserve_pool.pool().len());
            if pool.pool().is_empty() {
                return Err(Fail::new(libc::ENOMEM, "out of memory"));
            }
        }

        let headroom: u32 = u32::try_from(headroom).map_err(Fail::from)?;
        let umem: libxdp::XSK_UMEM_REG = libxdp::XSK_UMEM_REG {
            TotalSize: total_size,
            ChunkSize: real_chunk_size as u32,
            Headroom: headroom,
            Address: buffer_ptr.as_ptr() as *mut core::ffi::c_void,
        };

        // Register the UMEM region.
        trace!("registering umem region");
        socket.setsockopt(
            api,
            libxdp::XSK_SOCKOPT_UMEM_REG,
            &umem as *const libxdp::XSK_UMEM_REG as *const core::ffi::c_void,
            std::mem::size_of::<libxdp::XSK_UMEM_REG>() as u32,
        )?;

        Ok(Self {
            _buffer: buffer,
            pool,
            reserve_pool,
            umem,
            buf_offset_from_chunk,
        })
    }

    /// Get a buffer from the umem pool.
    pub fn get_buffer(&self, reserve: bool) -> Option<DemiBuffer> {
        if reserve {
            self.reserve_pool
                .as_ref()
                .and_then(|pool: &BufferPool| DemiBuffer::new_in_pool(pool))
        } else {
            DemiBuffer::new_in_pool(&self.pool)
        }
    }

    /// Returns a raw pointer to the the start address of the user memory region.
    pub fn address(&self) -> NonNull<u8> {
        // NB: non-nullness is validated by the constructor.
        NonNull::new(self.umem.Address.cast::<u8>()).unwrap()
    }

    /// Returns the region of memory that the umem region occupies.
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn is_data_in_pool(&self, buf: &DemiBuffer) -> bool {
        let data: NonNull<u8> = unsafe { NonNull::new_unchecked(buf.as_ptr() as *mut u8) };
        self.region().contains(&data)
    }

    /// Same as `self.dehydrate_buffer(self.get_buffer()?)`, but returns only the base address of
    /// the buffer. Useful for publishing to receive rings.
    pub fn get_dehydrated_buffer(&self, reserve: bool) -> Option<u64> {
        let buf: DemiBuffer = self.get_buffer(reserve)?;
        let desc: libxdp::XSK_BUFFER_DESCRIPTOR = self.dehydrate_buffer(buf);
        assert!(
            unsafe { desc.Address.__bindgen_anon_1.Offset() }
                == (self.overhead_bytes() as u64 + self.buf_offset_from_chunk as u64)
        );
        Some(unsafe { desc.Address.__bindgen_anon_1.BaseAddress() })
    }

    /// Dehydrates a DemiBuffer into a usize that can be rehydrated later. This operation consumes the DemiBuffer.
    pub fn dehydrate_buffer(&self, buf: DemiBuffer) -> libxdp::XSK_BUFFER_DESCRIPTOR {
        let data_len: usize = buf.len();
        let data: *const u8 = buf.as_ptr();
        let basis: NonNull<u8> = if buf.is_direct() {
            buf.into_raw()
        } else {
            let direct: DemiBuffer = buf.into_direct();
            direct.into_raw()
        };

        let basis: NonNull<u8> = unsafe { basis.offset(-self.buf_offset_from_chunk) };

        // Safety: MemoryPool guarantees that the DemiBuffer data is allocated from the allocated object pointed to
        // by `self.address()`.
        let base_address: usize = unsafe { basis.offset_from(self.address().cast::<u8>()) as usize };
        let offset: usize = unsafe { data.offset_from(basis.as_ptr()) as usize };

        if (self.umem.TotalSize - self.umem.ChunkSize as u64) < base_address as u64 {
            panic!("buffer {} not in region", base_address);
        }

        let addr: libxdp::XSK_BUFFER_ADDRESS = unsafe {
            let mut addr: libxdp::XSK_BUFFER_ADDRESS = std::mem::zeroed();
            addr.__bindgen_anon_1.set_BaseAddress(base_address as u64);
            addr.__bindgen_anon_1.set_Offset(offset as u64);
            addr
        };

        libxdp::XSK_BUFFER_DESCRIPTOR {
            Address: addr,
            Length: data_len as u32,
            Reserved: 0,
        }
    }

    /// Rehydrates a buffer from an XSK_BUFFER_DESCRIPTOR that was previously dehydrated by `dehydrate_buffer`.
    pub fn rehydrate_buffer_desc(&self, desc: &libxdp::XSK_BUFFER_DESCRIPTOR) -> Result<DemiBuffer, Fail> {
        self.rehydrate_buffer_offset(unsafe { desc.Address.__bindgen_anon_1.BaseAddress() })
            .and_then(|mut buf: DemiBuffer| -> Result<DemiBuffer, Fail> {
                buf.trim(buf.len().saturating_sub(desc.Length as usize))?;
                Ok(buf)
            })
    }

    /// Rehydrates a buffer from a usize that was previously dehydrated by `dehydrate_buffer`.
    pub fn rehydrate_buffer_offset(&self, offset: u64) -> Result<DemiBuffer, Fail> {
        let token: NonNull<u8> = unsafe { self.address().offset(isize::try_from(offset).map_err(Fail::from)?) };
        let demi_token: NonNull<u8> = unsafe { token.offset(self.buf_offset_from_chunk) };

        Ok(unsafe { DemiBuffer::from_raw(demi_token) })
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
