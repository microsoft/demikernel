// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use std::{
    alloc::{
        Layout,
        LayoutError,
    },
    cell::UnsafeCell,
    marker::PhantomData,
    mem::{
        ManuallyDrop,
        MaybeUninit,
    },
    num::NonZeroUsize,
    ops::{
        Deref,
        DerefMut,
    },
    ptr::NonNull,
    rc::Rc,
};

use crate::runtime::fail::Fail;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A single-threaded, page-aware pool of homogeneously-sized buffers.
#[derive(Debug)]
pub struct MemoryPool {
    buffers: UnsafeCell<Vec<NonNull<[MaybeUninit<u8>]>>>,

    buf_layout: Layout,
}

/// A buffer from a [`MemoryPool`]
#[derive(Debug)]
pub struct PoolBuf {
    buffer: NonNull<[MaybeUninit<u8>]>,
    pool: Rc<MemoryPool>,
}

/// This struct tracks consumption of a buffer, allowing the caller to take subspans and skip bytes, with useful
/// pointer-alignment methods.
struct BufferCursor {
    /// The pointer to the location of the cursor. Ideally, this would be stored as a slice, but the DST pointer support
    /// with non-null is underwhelming.
    cursor: NonNull<MaybeUninit<u8>>,

    /// Size of the memory span pointed to by cursor.
    len: usize,
}

/// An iterator which will pack objects of a specific layout into a series of memory pages. The algorithm will try to
/// minimize the number of pages each object spans, which may result in unused bytes at the end of each page. When the
/// aligned size is a factor or multiple of the page size, objects will be tightly packed.
struct PackingIterator<'a> {
    cursor: BufferCursor,
    layout: Layout,
    page_size: NonZeroUsize,
    _marker: PhantomData<&'a mut [MaybeUninit<u8>]>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

impl MemoryPool {
    /// Create a new empty pool of buffers of the specified size.
    pub fn new(size: NonZeroUsize, align: NonZeroUsize) -> Result<Rc<Self>, LayoutError> {
        Ok(Rc::new(Self {
            buffers: UnsafeCell::new(Vec::new()),
            buf_layout: Layout::from_size_align(size.get(), align.get())?,
        }))
    }

    /// Get the buffer layout for this pool.
    pub fn layout(self: &Rc<Self>) -> Layout {
        self.buf_layout
    }

    /// Get one buffer from the pool. If no buffers remain, returns None.
    pub fn get(self: &Rc<Self>) -> Option<PoolBuf> {
        let buffers: &mut Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &mut *self.buffers.get() };
        let pool: Rc<Self> = self.clone();
        buffers
            .pop()
            .map(|buffer: NonNull<[MaybeUninit<u8>]>| PoolBuf { buffer, pool })
    }

    /// Return a buffer to the pool.
    fn return_buffer(self: &Rc<Self>, buffer: NonNull<[MaybeUninit<u8>]>) {
        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &mut Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &mut *self.buffers.get() };
        buffers.push(buffer);
    }

    /// Populate the pool by adding buffers from a series of memory pages given by `buffer`. This method will pack
    /// buffers of the configured size into the pages such that each buffer spans the minimum number of pages. Returns
    /// any unused bytes after the final buffer.
    ///
    /// # Safety:
    /// The caller must ensure that `buffer` is a valid pointer which lives at least as long as the `MemoryPool`.
    pub unsafe fn populate(
        self: &Rc<Self>,
        mut buffer: NonNull<[MaybeUninit<u8>]>,
        page_size: NonZeroUsize,
    ) -> Result<NonNull<[MaybeUninit<u8>]>, Fail> {
        let mut iter: PackingIterator = PackingIterator::new(unsafe { buffer.as_mut() }, page_size, self.buf_layout)?;

        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &mut Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &mut *self.buffers.get() };
        buffers.extend(std::iter::from_fn(|| iter.next()).map(NonNull::from));

        Ok(NonNull::from(iter.into_slice()))
    }

    /// Returns the number of free buffers in the pool.
    pub fn len(self: &Rc<Self>) -> usize {
        // Safety: buffers is only granted a &mut alias during the methods of this class. As long as these methods are
        // neither called asynchronously nor nested, aliasing is obeyed.
        let buffers: &Vec<NonNull<[MaybeUninit<u8>]>> = unsafe { &*self.buffers.get() };
        buffers.len()
    }

    /// Returns a flag indicating whether the pool is out of buffers.
    pub fn is_empty(self: &Rc<Self>) -> bool {
        self.len() == 0
    }
}

impl PoolBuf {
    /// Get the pool associated with the buffer.
    pub fn pool(&self) -> Rc<MemoryPool> {
        self.pool.clone()
    }

    /// Consume a PoolBuf and return the underlying buffer and memory pool. The caller is responsible for returning the
    /// buffer to the memory pool by later calling [`PoolBuf::into_raw`]. Failing to call this method will result in the
    /// buffer leaking from the memory pool. Because the pool does not own the allocated memory, this may or may not
    /// result in a memory leak after the pool is deallocated.
    ///
    /// Note that this is an associated function to match idioms in e.g., [`Box::into_raw`].
    pub fn into_raw(b: PoolBuf) -> (NonNull<[MaybeUninit<u8>]>, Rc<MemoryPool>) {
        let b: ManuallyDrop<Self> = ManuallyDrop::new(b);

        // Safety: pool field is valid and readable.
        let pool: Rc<MemoryPool> = unsafe { std::ptr::read(&b.pool) };
        (b.buffer, pool)
    }

    /// Create a PoolBuf from a buffer pointer and memory pool reference previously returned from [`PoolBuf::into_raw`].
    ///
    /// # Safety:
    /// This method is only safe when called on values returned from [`PoolBuf::into_raw`], and only once for each call
    /// to that method.
    pub unsafe fn from_raw(buffer: NonNull<[MaybeUninit<u8>]>, pool: Rc<MemoryPool>) -> Self {
        PoolBuf { buffer, pool }
    }
}

impl BufferCursor {
    /// Create a new buffer cursor over the specified slice.
    pub fn new(buffer: &mut [MaybeUninit<u8>]) -> Self {
        // NB, we could support this, but it's not practically necessary.
        assert!(buffer.len() < isize::MAX as usize);
        Self {
            len: buffer.len(),
            cursor: NonNull::from(buffer).cast(),
        }
    }

    /// Reborrow the reference to the underlying buffer, creating a new cursor.
    pub fn reborrow(&mut self) -> Self {
        Self {
            cursor: self.cursor,
            len: self.len,
        }
    }

    /// Take at most `bytes` bytes starting at the cursor. If not enough bytes remain in the buffer, the returned value
    /// will be shorter than the requested number of bytes. Otherwise, the returned slice will have a length equal to
    /// `bytes`.
    pub fn take_at_most<'a>(&mut self, bytes: usize) -> &'a mut [MaybeUninit<u8>] {
        debug_assert!(bytes <= isize::MAX as usize);

        let bytes: usize = std::cmp::min(bytes, self.len);

        // Safety: the offset from cursor is within the originally allocated span and not larger than isize::MAX.
        let result: &mut [MaybeUninit<u8>] = unsafe { std::slice::from_raw_parts_mut(self.cursor.as_ptr(), bytes) };
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

    /// Try to take `size` bytes starting at the cursor. The cursor is updated iff the return value is not None. This
    /// method will return None if the remaining length of the buffer is less than `size`.
    pub fn try_take<'a>(&mut self, size: usize) -> Option<&'a mut [MaybeUninit<u8>]> {
        (size <= self.len).then(|| self.take_at_most(size))
    }

    /// Similar to [std::ptr::align_offset], but computes the next alignment after the cursor.
    pub fn next_align_offset(&self, align: NonZeroUsize) -> Option<NonZeroUsize> {
        (!self.is_empty())
            .then(|| unsafe { NonZeroUsize::new_unchecked(self.cursor.as_ptr().add(1).align_offset(align.get()) + 1) })
    }

    /// Whether any bytes remain at the cursor.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Consume `self` and produce a reference to the remaining buffer.
    pub fn into_slice<'a>(mut self) -> &'a mut [MaybeUninit<u8>] {
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
            _marker: PhantomData,
        })
    }

    /// Consume `self` and produce a reference to the remaining buffer.
    pub fn into_slice(self) -> &'a mut [MaybeUninit<u8>] {
        self.cursor.into_slice()
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl Drop for PoolBuf {
    fn drop(&mut self) {
        MemoryPool::return_buffer(&self.pool, self.buffer);
    }
}

impl Deref for PoolBuf {
    type Target = [MaybeUninit<u8>];

    fn deref(&self) -> &Self::Target {
        // Safety: the pointer is valid, as it comes from a valid slice reference. Since PoolBuf is a unique owner of
        // the buffer, aliasing rules are enforced automatically.
        unsafe { self.buffer.as_ref() }
    }
}

impl DerefMut for PoolBuf {
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
        let mut temp: BufferCursor = self.cursor.reborrow();

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

//======================================================================================================================
// Unit Tests
//======================================================================================================================

// Unit tests for `BufferPool` type.
#[cfg(test)]
mod tests {
    use std::{
        mem::MaybeUninit,
        num::NonZeroUsize,
        ptr::NonNull,
        rc::Rc,
    };

    use ::anyhow::{
        anyhow,
        Result,
    };
    use anyhow::ensure;

    use crate::{
        ensure_eq,
        runtime::memory::memory_pool::{
            MemoryPool,
            PoolBuf,
        },
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
        let pool: Rc<MemoryPool> = MemoryPool::new(buf_size_ea, buf_align)?;

        ensure_eq!(pool.len(), 0);
        ensure!(pool.get().is_none());

        let remaining: &mut [MaybeUninit<u8>] = unsafe { pool.populate(NonNull::from(buffer), page_size)?.as_mut() };
        ensure_eq!(remaining.len(), results.bytes_left_over);

        ensure_eq!(pool.len(), results.number_of_buffers);

        let mut bufs: Vec<Option<PoolBuf>> = Vec::from_iter(std::iter::from_fn(|| Some(pool.get())).take(pool.len()));

        ensure_eq!(bufs.len(), results.number_of_buffers);
        ensure!(bufs.iter().all(|o: &Option<_>| o.is_some()));
        ensure_eq!(pool.len(), 0);
        ensure!(pool.get().is_none());

        // Sort by address so we can validate the alignment of successive buffers.
        bufs.sort_by(|a: &Option<PoolBuf>, b: &Option<PoolBuf>| {
            a.as_ref().unwrap().as_ptr().cmp(&b.as_ref().unwrap().as_ptr())
        });

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

            ensure!(
                buf.as_ptr().addr() >= last_buffer_ptr + align && buf.as_ptr().addr() <= last_buffer_ptr + align + span
            );
            last_buffer_ptr = buf.as_ptr().addr();

            buf.fill(MaybeUninit::new(0xAAu8));
            std::mem::drop(buf);

            ensure_eq!(pool.len(), 1);

            // NB MemoryPool does not guarantee LIFO, but since the pool only has one buffer in it, it must be the same
            // buffer.
            let buf: PoolBuf = pool.get().ok_or(anyhow!("pool should not be empty"))?;
            ensure_eq!(expected, unsafe {
                std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len())
            });
            *buf_holder = Some(buf);
        }

        ensure_eq!(pool.len(), 0);
        ensure!(pool.get().is_none());

        if !bufs.is_empty() {
            bufs.pop().unwrap();
        }
        if !bufs.is_empty() {
            bufs.pop().unwrap();
        }

        ensure_eq!(pool.len(), std::cmp::min(2, results.number_of_buffers));

        if results.number_of_buffers > 0 {
            ensure!(pool.get().is_some());
        }

        std::mem::drop(bufs);

        ensure_eq!(pool.len(), results.number_of_buffers);
        ensure!(pool.get().is_some());

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
