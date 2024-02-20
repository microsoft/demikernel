// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use core::slice;
use std::{
    alloc::Layout,
    cell::UnsafeCell,
    mem::MaybeUninit,
    num::NonZeroUsize,
    ptr::NonNull,
    rc::Rc,
};

use crate::{
    pal::arch,
    runtime::{
        fail::Fail,
        memory::demibuffer::DemiBuffer,
    },
};

//======================================================================================================================
// Structures
//======================================================================================================================

/// Internal state controlled by [BufferPool] and [BufferPoolController].
struct BufferPoolState {
    buffers: Vec<NonNull<[u8]>>,
}

/// This structure provides a facility to preallocate DemiBuffers, then use the preallocated buffers on the data path.
/// Buffers are exposed via the [PooledBuffer] trait; this struct then interfaces with [DemiBuffer] to provide a safe
/// instance of the buffer, which is automatically returned to the ring when the DemiBuffer is dropped. The pool is
/// prepopulated with PooledBuffers and optionally configured with a factory which can construct additional buffers
/// if the pool is exhausted.
pub struct BufferPool {
    state: Rc<UnsafeCell<BufferPoolState>>,

    buffer_data_size: NonZeroUsize,
}

struct BufferCursor<'a>(&'a mut [MaybeUninit<u8>]);

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl BufferPoolState {
    pub fn try_checkout(&mut self) -> Option<NonNull<[u8]>> {
        self.buffers.pop()
    }

    pub fn return_buffer(&mut self, buffer: NonNull<[u8]>) {
        self.buffers.push(buffer);
    }
}

// impl<'a, Buffer: PooledBuffer> BufferPoolController<'a, Buffer> {
//     pub fn reserve_capacity(&self, additional: usize) {
//         self.0.buffers.reserve(additional);
//     }

//     pub fn add_buffer(&self, buffer: Buffer) {
//         // This might be changed to a different method in the future.
//         self.0.return_buffer(buffer);
//     }
// }

impl BufferPool {
    pub fn new(buffer_data_size: NonZeroUsize) -> Self {
        Self {
            state: Rc::new(UnsafeCell::new(BufferPoolState { buffers: Vec::new() })),
            buffer_data_size,
        }
    }

    pub fn populate<InitBufCb: FnMut(&mut DemiBuffer), FreeMem: FnOnce(NonNull<[u8]>)>(
        &mut self,
        mem: NonNull<[MaybeUninit<u8>]>,
        page_size: NonZeroUsize,
        init_buf_callback: InitBufCb,
        free_mem: FreeMem,
    ) -> Result<(), Fail> {
        if mem.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "memory buffer too short"));
        }

        if !page_size.is_power_of_two() || page_size.get() <= arch::CPU_DATA_CACHE_LINE_SIZE {
            return Err(Fail::new(libc::EINVAL, "page size is not valid"));
        }

        let buf_size_ea: NonZeroUsize = self.buffer_data_size.saturating_add(DemiBuffer::get_overhead());
        let buf_layout: Layout =
            Layout::from_size_align(buf_size_ea.get(), arch::CPU_DATA_CACHE_LINE_SIZE)?.pad_to_align();

        // The number of bytes required to be in a page to span the minimum number of pages.
        let req_bytes_in_page: usize = buf_layout.size() % page_size;

        let base: *mut u8 = mem.cast().as_ptr();
        let mut off: usize = base.align_offset(buf_layout.align());
        if off > mem.len() {
            // Note align_offset may return usize::MAX; however, since our pointer stride is 1 (size_of::<u8>()), that
            // case shouldn't happen. This block should only be triggered when the user passes artificially small blocks
            // of memory.
            return Err(Fail::new(libc::EINVAL, "cannot align buffers within the memory region"));
        }

        while off < mem.len() {
            let bytes_in_page: NonZeroUsize = unsafe {
                NonZeroUsize::new_unchecked(
                    base.wrapping_add(off + 1)
                        .align_offset(page_size.get())
                        .saturating_add(1),
                )
            };
            if bytes_in_page < page_size && bytes_in_page < req_bytes_in_page {
                off += bytes_in_page;
            }

            if off + buf_layout.size() >= mem.len() {
                break;
            }

            let mem_ptr: *mut u8 = base.wrapping_add(off);
            assert!(mem_ptr.is_aligned_to(arch::CPU_DATA_CACHE_LINE_SIZE));

            // Safety: memory is aligned and valid for the slice (as per the loop logic). MaybeUninit is not required to
            // be initialized.
            let buffer: &mut [MaybeUninit<u8>] = unsafe { slice::from_raw_parts_mut(mem_ptr, buf_layout.size()) };
            off += buf_layout.size();

            // TODO: EMIT BUFFER
        }

        Ok(())
    }

    pub fn populate_external(
        &mut self,
        metadata_buffer: &mut [MaybeUninit<u8>],
        metadata_page_size: usize,
        data_buffer: &mut [MaybeUninit<u8>],
        data_page_size: usize,
    ) -> Result<(), Fail> {
    }

    pub fn populate2<InitBufCb: FnMut(&mut DemiBuffer), FreeMem: FnOnce(NonNull<[u8]>)>(
        &mut self,
        buffer: &mut [MaybeUninit<u8>],
        page_size: NonZeroUsize,
        init_buf_callback: InitBufCb,
        free_mem: FreeMem,
    ) -> Result<(), Fail> {
        let buf_size_ea: NonZeroUsize = self.buffer_data_size.saturating_add(DemiBuffer::get_overhead());
        let layout: Layout = Layout::from_size_align(buf_size_ea.get(), arch::CPU_DATA_CACHE_LINE_SIZE)?;

        pack_in_pages(
            buffer,
            layout,
            page_size,
            |buffer: &mut [MaybeUninit<u8>]| -> Result<(), Fail> {
                // TODO: Emit buffer
                Ok(())
            },
        )
    }

    // /// Try to check out a DemiBuffer from the ring. This method has three possible return combinations:
    // /// - Ok(Some(buffer)) => a valid buffer was returned.
    // /// - Ok(None) => the ring is exhausted and the factory declined to provide additional buffers, or no factory is
    // ///               set.
    // /// - Err(err) => the ring is exhausted and the factory failed in creating additional buffers.
    // pub fn try_checkout(&mut self) -> Result<Option<DemiBuffer>, Fail> {
    //     // Safety: this is the only public-facing way to create a mutable reference to the state. As long as this method
    //     // does not cause a DemiBuffer holding a Buffer to drop, then only one mutable reference will be outstanding.
    //     let state: &mut BufferPoolState<Buffer> = unsafe { &mut *self.state.get() };
    //     let buffer: Option<Buffer> = match state.try_checkout() {
    //         Some(buffer) => Some(buffer),
    //         None => {
    //             {
    //                 let controller: BufferPoolController<Buffer> = BufferPoolController(state);
    //                 self.call_factory(controller)?;
    //             }

    //             // Safety: See above.
    //             state.try_checkout()
    //         },
    //     };

    //     match buffer {
    //         Some(buffer) => Ok(Some(self.wrap_buffer(buffer))),
    //         None => Ok(None),
    //     }
    // }

    // /// Check out a buffer from the ring. This is the safe as [try_checkout], except that the Ok(None) case is treated
    // /// as a failure.
    // pub fn checkout(&mut self) -> Result<DemiBuffer, Fail> {
    //     match self.try_checkout().transpose() {
    //         None => Err(Fail::new(libc::ENOBUFS, "no more buffers available")),
    //         Some(result) => result,
    //     }
    // }

    // /// Wrap a Buffer in a DemiBuffer.
    // fn wrap_buffer(self: &Rc<Self>, buffer: Buffer) -> DemiBuffer {
    //     let self_ref: Rc<Self> = self.clone();
    // }

    // fn call_factory(&mut self, controller: BufferPoolController<Buffer>) -> Result<(), Fail> {
    //     match self.factory.as_mut().unwrap()(controller) {
    //         Ok(more) => {
    //             if !more {
    //                 self.factory.take();
    //             }
    //             Ok(())
    //         },
    //         Err(err) => Err(err),
    //     }
    // }
}

fn pack_in_pages<F>(buffer: &mut [MaybeUninit<u8>], layout: Layout, page_size: usize, emit: F) -> Result<(), Fail>
where
    F: FnOnce(&mut [MaybeUninit<u8>]) -> Result<(), Fail>,
{
    if buffer.len() == 0 {
        return Err(Fail::new(libc::EINVAL, "memory buffer too short"));
    }

    if !page_size.is_power_of_two() || page_size < layout.align() {
        return Err(Fail::new(libc::EINVAL, "page size is not valid"));
    }

    // The number of bytes required to be in a page to span the minimum number of pages.
    let req_bytes_in_page: usize = layout.size() % page_size;

    let cursor: BufferCursor = BufferCursor::new(buffer);

    loop {
        if !cursor.align_to(layout.align()) {
            break;
        }

        // NB req_bytes_in_page < page_size
        let next_page_align: NonZeroUsize = cursor.next_align_offset(page_size);
        if next_page_align < req_bytes_in_page {
            if !cursor.skip(next_page_align) {
                break;
            }
        }

        if let Some(buffer) = cursor.take(layout.size()) {
            if let Err(err) = emit(buffer) {
                return Err(err);
            }
        } else {
            break;
        }
    }

    Ok(())
}

impl<'a> BufferCursor<'a> {
    pub fn new(buffer: &'a mut [MaybeUninit<u8>]) -> Self {
        Self(buffer)
    }

    pub fn skip(&mut self, bytes: usize) -> bool {
        if bytes <= self.0.len() {
            self.0 = &mut self.0[bytes..];
            true
        } else {
            false
        }
    }

    pub fn align_to(&mut self, align: usize) -> bool {
        let bytes: usize = self.0.as_ptr().align_offset(align);
        self.skip(bytes)
    }

    pub fn take(&mut self, size: usize) -> Option<&mut [MaybeUninit<u8>]> {
        if size < self.0.len() {
            let (result, rest) = self.0.split_at_mut(size);
            self.0 = rest;
            Some(result)
        } else {
            None
        }
    }

    /// Return the offset require to advance the cursor to the next address which is aligned to `align`.
    pub fn next_align_offset(&self, align: usize) -> Option<NonZeroUsize> {
        (!self.0.is_empty())
            .then(|| unsafe { NonZeroUsize::new_unchecked(self.0.start.add(1).algin_offset(align) + 1) })
    }

    fn len(&self) -> usize {
        unsafe { self.end.sub_ptr(self.0.start) }
    }
}

// impl<Buffer: PooledBuffer> PooledBufferRef<Buffer> {
//     /// Decompose the pooled buffer reference into its raw parts.
//     pub fn into_raw(mut self) -> (NonNull<[u8]>, Buffer::Metadata, usize) {
//         let (slice, metadata) = Buffer::into_raw(self.0);
//         (slice, metadata, Rc::into_raw(self.1).addr())
//     }
// }
