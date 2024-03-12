// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{
    self,
    alloc::{
        Layout,
        LayoutError,
    },
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    num::NonZeroUsize,
    ops::{
        Deref,
        DerefMut,
    },
    ptr::NonNull,
};

use crate::runtime::fail::Fail;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A single-threaded, page-aware pool of homogeneously-sized buffers.
pub struct MemoryPool {
    buffers: UnsafeCell<Vec<NonNull<[MaybeUninit<u8>]>>>,

    layout: Layout,
}

/// A buffer from a [`MemoryPool`]
pub struct PoolBuf<'a> {
    buffer: NonNull<[MaybeUninit<u8>]>,
    pool: NonNull<MemoryPool>,
    _marker: PhantomData<&'a MemoryPool>,
}

/// This struct tracks consumption of a buffer, allowing the caller to take subspans and skip bytes, with useful
/// pointer-alignment methods.
struct BufferCursor<'a> {
    /// The pointer to the location of the cursor. Ideally, this would be stored as a slice, but the DST pointer support
    /// with non-null is underwhelming.
    cursor: NonNull<MaybeUninit<u8>>,

    /// Size of the memory span pointed to by cursor.
    len: usize,

    /// Ensure lifetime reference to the original span.
    _marker: PhantomData<&'a mut [MaybeUninit<u8>]>,
}

/// An iterator which will pack objects of a specific layout into a series of memory pages. The algorithm will try to
/// minimize the number of pages each object spans, which may result in unused bytes at the end of each page. When the
/// aligned size is a factor or multiple of the page size, objects will be tightly packed.
struct PackingIterator<'a> {
    cursor: BufferCursor<'a>,
    layout: Layout,
    page_size: NonZeroUsize,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl MemoryPool {
    /// Create a new empty pool of buffers of the specified size.
    pub fn new(size: NonZeroUsize, align: NonZeroUsize) -> Result<Self, LayoutError> {
        Ok(Self {
            buffers: UnsafeCell::new(Vec::new()),
            layout: Layout::from_size_align(size.get(), align.get())?,
        })
    }

    /// Get one buffer from the pool. If no buffers remain, returns None.
    pub fn get_one<'a>(&'a self) -> Option<PoolBuf<'a>> {
        self.get_iter(1).next()
    }

    /// Get `n` buffers from the pool, extending `out`. If out is extended by fewer than `n` buffers, the pool was
    /// exhausted.
    pub fn get_n<'a, C: Extend<PoolBuf<'a>>>(&'a self, n: usize, out: &mut C) {
        out.extend(self.get_iter(n));
    }

    /// Called by get_one and get_n to get a draining iterator to the underlying buffer vector.
    fn get_iter<'a>(&'a self, n: usize) -> impl Iterator<Item = PoolBuf<'a>> {
        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &mut Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &mut *self.buffers.get() };
        let n: usize = std::cmp::min(n, buffers.len());
        let pool: NonNull<Self> = NonNull::<Self>::from(self);
        buffers
            .drain((buffers.len() - n)..)
            .map(move |buffer: NonNull<[MaybeUninit<u8>]>| PoolBuf::<'a> {
                // Safety: buffer comes from a reference; lifetime is enforced by the `populate` trait bounds.
                buffer: buffer,
                pool,
                _marker: PhantomData,
            })
    }

    /// Return a buffer to the pool.
    fn return_buffer<'a>(buf: &mut PoolBuf<'a>) {
        // Safety: this pointer comes from a reference, so pointer rules are obeyed. Aliasing is enforced by the
        // PhantomData in PoolBuf.
        let me: &'a Self = unsafe { buf.pool.as_ref() };

        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &mut Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &mut *me.buffers.get() };
        buffers.push(buf.buffer);
    }

    /// Populate the pool by adding buffers from a series of memory pages given by `buffer`. This method will pack
    /// buffers of the configured size into the pages such that each buffer spans the minimum number of pages. Returns
    /// any unused bytes after the final buffer.
    pub fn populate<'a, 'b: 'a>(
        &'a self,
        buffer: &'b mut [MaybeUninit<u8>],
        page_size: NonZeroUsize,
    ) -> Result<&'a mut [MaybeUninit<u8>], Fail> {
        let mut iter: PackingIterator<'a> = PackingIterator::new(buffer, page_size, self.layout)?;

        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &mut Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &mut *self.buffers.get() };
        buffers.extend(std::iter::from_fn(|| iter.next()).map(NonNull::from));

        Ok(iter.into_slice())
    }

    /// Returns the number of free buffers in the pool.
    pub fn len(&self) -> usize {
        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &*self.buffers.get() };
        buffers.len()
    }
}

impl<'a> BufferCursor<'a> {
    pub fn new(buffer: &'a mut [MaybeUninit<u8>]) -> Self {
        // NB, we could support this, but it's not practically necessary.
        assert!(buffer.len() < isize::MAX as usize);
        Self {
            len: buffer.len(),
            cursor: NonNull::from(buffer).cast(),
            _marker: PhantomData,
        }
    }

    /// Reborrow the reference to the underlying buffer, creating a new cursor.
    pub fn reborrow(&mut self) -> Self {
        Self {
            cursor: self.cursor,
            len: self.len,
            _marker: PhantomData,
        }
    }

    /// Take at most `bytes` bytes starting at the cursor. If not enough bytes remain in the buffer, the returned value
    /// will be shorter than the requested number of bytes. Otherwise, the returned slice will have a length equal to
    /// `bytes`.
    pub fn take_at_most(&mut self, bytes: usize) -> &'a mut [MaybeUninit<u8>] {
        debug_assert!(bytes <= isize::MAX as usize);

        let bytes: usize = std::cmp::min(bytes, self.len);

        // Safety: the offset from cursor is within the originally allocated span and not larger than isize::MAX.
        let result: &'a mut [MaybeUninit<u8>] = unsafe { std::slice::from_raw_parts_mut(self.cursor.as_ptr(), bytes) };
        self.cursor = unsafe { NonNull::new_unchecked(self.cursor.as_ptr().offset(bytes as isize)) };
        self.len -= bytes;
        result
    }

    /// Skip at most `bytes` bytes. Returns true iff `bytes` were skipped and the cursor points to at least one byte.
    /// `bytes` must be no larger than [`isize::MAX`].
    pub fn skip(&mut self, bytes: usize) -> bool {
        self.take_at_most(bytes).len() == bytes
    }

    /// Align the cursor to `align`, skipping at most align bytes. Returns true iff the cursor is aligned to `align` and
    /// points to at least one byte.
    pub fn skip_to_align(&mut self, align: usize) -> bool {
        let bytes: usize = self.cursor.as_ptr().align_offset(align);
        self.skip(bytes)
    }

    /// Discards the rest of the buffer.
    pub fn skip_rest(&mut self) {
        self.skip(self.len);
    }

    /// Try to take `size` bytes starting at the cursor. The cursor is updated iff the return value is not None. This
    /// method will return None if the remaining length of the buffer is less than `size`.
    pub fn try_take(&mut self, size: usize) -> Option<&'a mut [MaybeUninit<u8>]> {
        (size <= self.len).then(|| self.take_at_most(size))
    }

    /// Similar to [std::ptr::align_offset], but computes the next alignment after the cursor.
    pub fn next_align_offset(&self, align: NonZeroUsize) -> Option<NonZeroUsize> {
        (!self.is_empty())
            .then(|| unsafe { NonZeroUsize::new_unchecked(self.cursor.as_ptr().add(1).align_offset(align.get()) + 1) })
    }

    /// The remaining buffer length from the cursor.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether any bytes remain at the cursor.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Consume `self` and produce a reference to the remaining buffer.
    pub fn into_slice(mut self) -> &'a mut [MaybeUninit<u8>] {
        self.take_at_most(self.len)
    }
}

impl<'a> PackingIterator<'a> {
    pub fn new(buffer: &'a mut [MaybeUninit<u8>], page_size: NonZeroUsize, layout: Layout) -> Result<Self, Fail> {
        if buffer.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "memory buffer too short"));
        }

        if !page_size.is_power_of_two() || page_size.get() < layout.align() {
            return Err(Fail::new(libc::EINVAL, "page size is not valid"));
        }

        Ok(Self {
            cursor: BufferCursor::new(buffer),
            layout: layout,
            page_size: page_size,
        })
    }

    /// Consume `self` and produce a reference to the remaining buffer.
    pub fn into_slice(self) -> &'a mut [MaybeUninit<u8>] {
        self.cursor.into_slice()
    }
}

impl<'a> Drop for PoolBuf<'a> {
    fn drop(&mut self) {
        MemoryPool::return_buffer(self);
    }
}

impl<'a> Deref for PoolBuf<'a> {
    type Target = [MaybeUninit<u8>];

    fn deref(&self) -> &Self::Target {
        // Safety: the pointer is valid, as it comes from a valid slice reference. Since PoolBuf is a unique owner of
        // the buffer, aliasing rules are enforced automatically.
        unsafe { self.buffer.as_ref() }
    }
}

impl<'a> DerefMut for PoolBuf<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: the pointer is valid, as it comes from a valid slice reference. Since PoolBuf is a unique owner of
        // the buffer, aliasing rules are enforced automatically.
        unsafe { self.buffer.as_mut() }
    }
}

impl<'a> Iterator for PackingIterator<'a> {
    type Item = &'a mut [MaybeUninit<u8>];

    fn next(&mut self) -> Option<&'a mut [MaybeUninit<u8>]> {
        // Reborrow cursor into a temporary so we can back out our changes if we fail.
        let mut temp: BufferCursor<'_> = self.cursor.reborrow();

        if !temp.skip_to_align(self.layout.align()) {
            return None;
        }

        // The number of bytes required to be in a page to span the minimum number of pages. The algorithm here
        // prioritizes minimizing the number of pages per object, which can result in sparse "packing".
        let req_bytes_in_page: usize = self.layout.size() % self.page_size;

        // Check how many bytes left in the page; see if we need to realign to reduce page spanning.
        match temp.next_align_offset(self.page_size) {
            Some(next_page_align) => {
                if next_page_align.get() < req_bytes_in_page {
                    // Need to align to prevent the next object from spanning an extra page.
                    if !temp.skip(next_page_align.get()) {
                        // Transitively, if we need to align to the next page but can't, we also can't fit  any more
                        // objects.
                        return None;
                    }
                }
            },

            // Cursor is empty.
            None => return None,
        };

        if let Some(buffer) = temp.try_take(self.layout.size()) {
            // Commit the cursor update.
            self.cursor = temp;
            Some(buffer)
        } else {
            None
        }
    }
}

// Unit tests for `BufferPool` type.
#[cfg(test)]
mod tests {
    use std::{
        mem::MaybeUninit,
        num::NonZeroUsize,
    };

    use ::anyhow::{
        anyhow,
        Result,
    };
    use anyhow::{
        bail,
        ensure,
    };

    use crate::ensure_eq;

    use super::{
        MemoryPool,
        PoolBuf,
    };

    fn alloc_page_buf(page_size: usize, alloc_size: usize, store: &mut Vec<MaybeUninit<u8>>) -> &mut [MaybeUninit<u8>] {
        store.clear();
        store.reserve(alloc_size + page_size);
        store.extend(std::iter::repeat(MaybeUninit::<u8>::zeroed()).take(alloc_size + page_size));

        let align_bytes: usize = store.as_ptr().align_offset(page_size);
        assert!(align_bytes + alloc_size <= store.len());
        &mut store.as_mut_slice()[align_bytes..alloc_size + align_bytes]
    }

    struct BasicTestSettings {
        page_size: usize,
        pool_size: usize,
        buf_size_ea: usize,
        buf_align: usize,
    }

    struct BasicTestResults {
        number_of_buffers: usize,
        bytes_left_over: usize,
    }

    fn run_basic_test(settings: BasicTestSettings, results: BasicTestResults) -> Result<()> {
        let page_size: NonZeroUsize = NonZeroUsize::new(settings.page_size).ok_or(anyhow!("bad page size"))?;
        let buf_size_ea: NonZeroUsize = NonZeroUsize::new(settings.buf_size_ea).ok_or(anyhow!("bad buffer size"))?;
        let buf_align: NonZeroUsize = NonZeroUsize::new(settings.buf_align).ok_or(anyhow!("bad buffer alignment"))?;

        let mut store: Vec<MaybeUninit<u8>> = Vec::new();
        let buffer: &mut [MaybeUninit<u8>] = alloc_page_buf(settings.page_size, settings.pool_size, &mut store);
        let pool: MemoryPool = MemoryPool::new(buf_size_ea, buf_align)?;

        ensure_eq!(pool.len(), 0);
        ensure!(pool.get_one().is_none());

        let remaining: &mut [MaybeUninit<u8>] = pool.populate(buffer, page_size)?;
        ensure_eq!(remaining.len(), results.bytes_left_over);

        ensure_eq!(pool.len(), results.number_of_buffers);

        let mut bufs: Vec<Option<PoolBuf>> = {
            let mut bufs: Vec<PoolBuf> = vec![];
            pool.get_n(results.number_of_buffers, &mut bufs);
            bufs.into_iter().map(|e| Some(e)).collect::<Vec<Option<PoolBuf>>>()
        };

        ensure_eq!(bufs.len(), results.number_of_buffers);
        ensure_eq!(pool.len(), 0);
        ensure!(pool.get_one().is_none());

        let expected: Vec<u8> = Vec::from_iter(std::iter::repeat(0xAAu8).take(buf_size_ea.get()));

        // NB if the buffer size is a factor or multiple of the page size, no bytes will be wasted at the end of the
        // page.
        let span: usize = if settings.buf_size_ea.is_power_of_two() {
            0
        } else {
            settings.buf_size_ea % settings.page_size
        };
        let align: usize = std::cmp::max(settings.buf_size_ea, settings.buf_align);
        let mut last_buffer_ptr: usize = bufs[0].as_ref().unwrap().as_ptr().addr() - align;

        for buf_holder in bufs.iter_mut() {
            let mut buf: PoolBuf = buf_holder.take().unwrap();
            ensure_eq!(buf.len(), buf_size_ea.get());
            if buf.as_ptr().addr() < last_buffer_ptr + align || buf.as_ptr().addr() > last_buffer_ptr + align + span {
                bail!("bad alignment");
            }
            // ensure!(
            //     buf.as_ptr().addr() >= last_buffer_ptr + buf_size_ea.get()
            //         && buf.as_ptr().addr() < last_buffer_ptr + buf_size_ea.get() * 2
            // );
            last_buffer_ptr = buf.as_ptr().addr();

            buf.fill(MaybeUninit::new(0xAAu8));
            std::mem::drop(buf);

            ensure_eq!(pool.len(), 1);

            // NB MemoryPool does not guarantee LIFO, but since the pool only has one buffer in it, it must be the same
            // buffer.
            let buf: PoolBuf = pool.get_one().ok_or(anyhow!("pool should not be empty"))?;
            ensure_eq!(expected, unsafe {
                std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len())
            });
            *buf_holder = Some(buf);
        }

        ensure_eq!(pool.len(), 0);
        ensure!(pool.get_one().is_none());

        if !bufs.is_empty() {
            bufs.pop().unwrap();
        }
        if !bufs.is_empty() {
            bufs.pop().unwrap();
        }

        ensure_eq!(pool.len(), std::cmp::min(2, results.number_of_buffers));

        if results.number_of_buffers > 0 {
            ensure!(pool.get_one().is_some());
        }

        std::mem::drop(bufs);

        ensure_eq!(pool.len(), results.number_of_buffers);
        ensure!(pool.get_one().is_some());

        Ok(())
    }

    #[test]
    fn test_small_power_2_one_page() -> Result<()> {
        run_basic_test(
            BasicTestSettings {
                pool_size: 128,
                page_size: 128,
                buf_size_ea: 32,
                buf_align: 1,
            },
            BasicTestResults {
                number_of_buffers: 4,
                bytes_left_over: 0,
            },
        )
    }

    #[test]
    fn test_small_power_2_many_pages() -> Result<()> {
        run_basic_test(
            BasicTestSettings {
                pool_size: 2048,
                page_size: 128,
                buf_size_ea: 32,
                buf_align: 1,
            },
            BasicTestResults {
                number_of_buffers: 64,
                bytes_left_over: 0,
            },
        )
    }

    #[test]
    fn test_small_odd_size_many_pages() -> Result<()> {
        run_basic_test(
            BasicTestSettings {
                pool_size: 2048,
                page_size: 128,
                buf_size_ea: 59,
                buf_align: 1,
            },
            BasicTestResults {
                number_of_buffers: 32,
                bytes_left_over: 10,
            },
        )
    }

    #[test]
    fn test_small_power_2_overaligned() -> Result<()> {
        run_basic_test(
            BasicTestSettings {
                pool_size: 2048,
                page_size: 128,
                buf_size_ea: 32,
                buf_align: 64,
            },
            BasicTestResults {
                number_of_buffers: 32,
                bytes_left_over: 32,
            },
        )
    }

    #[test]
    fn test_large_power_2_one_page() -> Result<()> {
        run_basic_test(
            BasicTestSettings {
                pool_size: 128,
                page_size: 128,
                buf_size_ea: 128,
                buf_align: 1,
            },
            BasicTestResults {
                number_of_buffers: 1,
                bytes_left_over: 0,
            },
        )
    }

    #[test]
    fn test_large_power_2_many_pages() -> Result<()> {
        run_basic_test(
            BasicTestSettings {
                pool_size: 2048,
                page_size: 128,
                buf_size_ea: 256,
                buf_align: 1,
            },
            BasicTestResults {
                number_of_buffers: 8,
                bytes_left_over: 0,
            },
        )
    }

    #[test]
    fn test_large_odd_size_many_pages() -> Result<()> {
        // NB 2048 bytes could fit 9 227-byte buffers if they were packed; each buffer here will get over-aligned to
        // 256 bytes to prevent any buffer spanning three pages.
        run_basic_test(
            BasicTestSettings {
                pool_size: 2048,
                page_size: 128,
                buf_size_ea: 227,
                buf_align: 1,
            },
            BasicTestResults {
                number_of_buffers: 8,
                bytes_left_over: 29,
            },
        )
    }
}
