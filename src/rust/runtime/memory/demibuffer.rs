// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This DemiBuffer type is designed to be a common abstraction defining the behavior of data buffers in Demikernel.
// It currently supports two underlying types of buffers: heap-allocated and DPDK-allocated.  The basic operations on
// DemiBuffers are designed to have equivalent behavior (effects on the data), regardless of the underlying buffer type.
// In particular, len(), adjust(), trim(), clone(), and split_back() are designed to behave the same regardless.
//
// The constructors/destructors, however, are necessarily different.  For DPDK-allocated buffers, a MBuf is expected
// to be allocated externally and provided to the DemiBuffer's "from_mbuf" constructor.  A MBuf can also be extracted
// from a DPDK allocated DemiBuffer via the "into_mbuf" routine.
//
// Note: if compiled without the "libdpdk" feature defined, the DPDK-specific functionality won't be present.

// Note on buffer chain support:
// DPDK has a concept of MBuf chaining where multiple MBufs may be linked together to form a "packet".  While the
// DemiBuffer routines for heap-allocated buffers also now support this functionality, it isn't yet exposed via the
// DemiBuffer interface.
// TODO: Expose buffer chain support once we have a solid use case.

// Note on intrusive queueing:
// Since all DemiBuffer types keep the metadata for each "view" in a separate allocated region, they can be queued
// using intrusive links (i.e. have a link field in the metadata).
// TODO: Expose calls to get/set a linking field.

// Note on the allocation functions:
// This code currently uses std::alloc() and std::dealloc() to allocate/free things from the heap.  Note that the Rust
// documentation says that these functions are expected to be deprecated in favor of their respective methods of the
// "Global" type when it and the "Allocator" trait become stable.

use crate::{
    pal::arch,
    runtime::{
        fail::Fail,
        memory::{
            buffer_pool::BufferPool,
            memory_pool::{
                MemoryPool,
                PoolBuf,
            },
        },
    },
};
#[cfg(feature = "libdpdk")]
use ::dpdk_rs::{
    rte_mbuf,
    rte_mempool,
    rte_pktmbuf_adj,
    rte_pktmbuf_clone,
    rte_pktmbuf_free,
    rte_pktmbuf_trim,
    rte_pktmbuf_prepend,
};
use ::std::{
    alloc::{
        alloc,
        dealloc,
        handle_alloc_error,
        Layout,
    },
    marker::PhantomData,
    mem::{
        self,
        size_of,
        MaybeUninit,
    },
    num::NonZeroUsize,
    ops::{
        BitOr,
        Deref,
        DerefMut,
    },
    ptr::{
        self,
        null_mut,
        NonNull,
    },
    slice,
};
use std::rc::Rc;

// Buffer Metadata.
// This is defined to match a DPDK MBuf (rte_mbuf) in order to potentially use the same code for some DemiBuffer
// operations that currently use identical (but separate) implementations for heap vs DPDK allocated buffers.
// Fields beginning with an underscore are not directly used by the current DemiBuffer implementation.
// Should be cache-line aligned (64 bytes on x86 or x86_64) and consume 2 cache lines (128 bytes on x86 or x86_64).
// Unfortunately, we have to use a numeric literal value in #[repr(align())] below, and can't use a defined constant.
// So we make an educated guess and then check that it matches that of our target_arch in a compile-time assert below.
#[repr(C)]
#[repr(align(64))]
#[derive(Debug)]
pub(super) struct MetaData {
    // Virtual address of the start of the actual data.
    buf_addr: *mut u8,

    // Physical address of the buffer.
    _buf_iova: MaybeUninit<u64>,

    // Data offset.
    data_off: u16,
    // Reference counter.
    refcnt: u16,
    // Number of segments in this buffer chain (only valid in first segment's MetaData).
    nb_segs: u16,
    // Input port.
    _port: MaybeUninit<u16>,

    // Offload features.
    // Note, despite the "offload" name, the indirect buffer flag (METADATA_F_INDIRECT) lives here.
    ol_flags: u64,

    // L2/L3/L4 and tunnel information.
    _packet_type: MaybeUninit<u32>,
    // Total packet data length (sum of all segments' data_len).
    pkt_len: u32,

    // Amount of data in this segment buffer.
    data_len: u16,
    // VLAN TCI.
    _vlan_tci: MaybeUninit<u16>,
    // Potentially used for various things, including RSS hash.
    _various1: MaybeUninit<u32>,

    // Potentially used for various things, including RSS hash.
    _various2: MaybeUninit<u32>,
    _vlan_tci_outer: MaybeUninit<u16>,
    // Allocated length of the buffer that buf_addr points to.
    buf_len: u16,

    // Pointer to memory pool (rte_mempool) from which mbuf was allocated.
    pool: Option<Rc<MemoryPool>>,

    // Second cache line (64 bytes) begins here.

    // Pointer to the MetaData of the next segment in this packet's chain (must be NULL in last segment).
    next: Option<NonNull<MetaData>>,

    // Various fields for TX offload.
    _tx_offload: MaybeUninit<u64>,

    // Pointer to shared info. Used to manage external buffers.
    _shinfo: MaybeUninit<u64>,

    // Size of private data (between rte_mbuf struct and the data) in direct MBufs.
    _priv_size: MaybeUninit<u16>,
    // Timesync flags for use with IEEE 1588 "Precision Time Protocol" (PTP).
    _timesync: MaybeUninit<u16>,
    // Reserved for dynamic fields.
    _dynfield: MaybeUninit<[u32; 9]>,
}

/// The minimal set of metadata used by the Demikernel runtime. Used to safely initialize MetaData.
struct DemiMetaData {
    // Virtual address of the start of the actual data.
    buf_addr: *mut u8,

    // Data offset.
    data_off: u16,

    // Reference counter.
    refcnt: u16,

    // Number of segments in this buffer chain (only valid in first segment's MetaData).
    nb_segs: u16,

    // Offload features.
    // Note, despite the "offload" name, the indirect buffer flag (METADATA_F_INDIRECT) lives here.
    ol_flags: u64,

    // Total packet data length (sum of all segments' data_len).
    pkt_len: u32,

    // Amount of data in this segment buffer.
    data_len: u16,

    // Allocated length of the buffer that buf_addr points to.
    buf_len: u16,

    // Pointer to memory pool (rte_mempool) from which mbuf was allocated.
    pool: Option<Rc<MemoryPool>>,

    // Pointer to the MetaData of the next segment in this packet's chain (must be NULL in last segment).
    next: Option<NonNull<MetaData>>,
}

// Check MetaData structure alignment and size at compile time.
// Note that alignment must be specified via a literal value in #[repr(align(64))] above, so a defined constant can't
// be used.  So, if the alignment assert is firing, change the value in the align() to match CPU_DATA_CACHE_LINE_SIZE.
const _: () = assert!(std::mem::align_of::<MetaData>() == arch::CPU_DATA_CACHE_LINE_SIZE);
const _: () = assert!(std::mem::size_of::<MetaData>() == 2 * arch::CPU_DATA_CACHE_LINE_SIZE);
const _: () = assert!(std::mem::size_of::<Option<Rc<MemoryPool>>>() == std::mem::size_of::<*const ()>());

// MetaData "offload flags".  These exactly mimic those of DPDK MBufs.

// Indicates this MetaData struct doesn't have the actual data directly attached, but rather this MetaData's buf_addr
// points to another MetaData's directly attached data.
const METADATA_F_INDIRECT: u64 = 1 << 62;

impl MetaData {
    // Note on Reference Counts:
    // Since we are currently single-threaded, there is no need to use atomic operations for refcnt manipulations.
    // We should rework the implementation of inc_refcnt() and dec_refcnt() to use atomic operations if this changes.
    // Also, we intentionally don't check for refcnt overflow.  This matches DPDK's behavior, which doesn't check for
    // reference count overflow either (we're highly unlikely to ever have 2^16 copies of the same data).

    // Hydrate a MetaData instance from the subset of values used by Demikernel.
    fn new(values: DemiMetaData) -> Self {
        MetaData {
            buf_addr: values.buf_addr,
            data_off: values.data_off,
            refcnt: values.refcnt,
            nb_segs: values.nb_segs,
            ol_flags: values.ol_flags,
            pkt_len: values.pkt_len,
            data_len: values.data_len,
            buf_len: values.buf_len,
            pool: values.pool,
            next: values.next,

            // Unused fields
            _buf_iova: MaybeUninit::uninit(),
            _port: MaybeUninit::uninit(),
            _packet_type: MaybeUninit::uninit(),
            _shinfo: MaybeUninit::uninit(),
            _vlan_tci: MaybeUninit::uninit(),
            _various1: MaybeUninit::uninit(),
            _various2: MaybeUninit::uninit(),
            _vlan_tci_outer: MaybeUninit::uninit(),
            _tx_offload: MaybeUninit::uninit(),
            _timesync: MaybeUninit::uninit(),
            _dynfield: MaybeUninit::uninit(),

            // Initialize select MetaData fields in debug builds for sanity checking.
            // We check in debug builds that they aren't accidentally messed with.
            _priv_size: if cfg!(debug_assertions) {
                MaybeUninit::new(0)
            } else {
                MaybeUninit::uninit()
            },
        }
    }

    // Increments the reference count and returns the new value.
    #[inline]
    fn inc_refcnt(&mut self) -> u16 {
        self.refcnt += 1;
        self.refcnt
    }

    // Decrements the reference count and returns the new value.
    #[inline]
    fn dec_refcnt(&mut self) -> u16 {
        // We should never decrement an already zero reference count.  Check this on debug builds.
        debug_assert_ne!(self.refcnt, 0);
        self.refcnt -= 1;
        self.refcnt
    }

    // Gets the MetaData for the last segment in the buffer chain.
    #[inline]
    fn get_last_segment(&mut self) -> &mut MetaData {
        let mut md: &mut MetaData = self;
        while md.next.is_some() {
            // Safety: The call to as_mut is safe, as the pointer is aligned and dereferenceable, and the MetaData
            // struct it points to is initialized properly.
            md = unsafe { md.next.unwrap().as_mut() };
        }
        &mut *md
    }
}

// DemiBuffer type tags.
// Since our MetaData structure is 64-byte aligned, the lower 6 bits of a pointer to it are guaranteed to be zero.
// We currently only use the lower 2 of those bits to hold the type tag.
#[derive(PartialEq)]
enum Tag {
    Heap = 1,
    #[cfg(feature = "libdpdk")]
    Dpdk = 2,
}

impl Tag {
    const MASK: usize = 0x3;
}

impl BitOr<Tag> for NonZeroUsize {
    type Output = Self;

    fn bitor(self, rhs: Tag) -> Self::Output {
        self | rhs as usize
    }
}

impl From<usize> for Tag {
    fn from(tag_value: usize) -> Self {
        match tag_value {
            1 => Tag::Heap,
            #[cfg(feature = "libdpdk")]
            2 => Tag::Dpdk,
            _ => panic!("memory corruption in DemiBuffer pointer"),
        }
    }
}

/// The `DemiBuffer`.
///
/// This buffer type is designed to be a common abstraction defining the behavior of data buffers in Demikernel.
/// It currently supports two underlying types of buffers: heap-allocated and DPDK-allocated.  It defines the basic
/// operations on `DemiBuffer`s; these have the same effect on the data regardless of the underlying buffer type.
#[derive(Debug)]
pub struct DemiBuffer {
    // Pointer to the buffer metadata.
    // Stored as a NonNull so it can efficiently be packed into an Option.
    // This is a "tagged pointer" where the lower bits encode the type of buffer this points to.
    tagged_ptr: NonNull<MetaData>,
    // Hint to compiler that this struct "owns" a MetaData (for safety determinations).  Doesn't consume space.
    _phantom: PhantomData<MetaData>,
}

// Safety: Technically, DemiBuffer's aren't safe to Send between threads in their current implementation, as the
// reference counting on the data region isn't performed using (expensive) atomic operations, for performance reasons.
// This is okay in practice, as we currently run Demikernel single-threaded.  If this changes, the reference counting
// functionality (MetaData's inc_refcnt() and dec_refcnt(), see above) will need to be swapped out for ones that use
// atomic operations.  And then it will be safe to mark DemiBuffer as Send.
//
// TODO: For now, this is here because some of our test infrastructure wants to send DemiBuffers to other threads.
unsafe impl Send for DemiBuffer {}

impl DemiBuffer {
    // ------------
    // Constructors
    // ------------

    /// Creates a new (Heap-allocated) `DemiBuffer`.

    // Implementation Note:
    // This function is replacing the new() function of DataBuffer, which could return failure.  However, the only
    // failure it actually reported was if the new DataBuffer request was for zero size.  A separate empty() function
    // was provided to allocate zero-size buffers.  This new implementation does not have a special case for this,
    // instead, zero is a valid argument to new().  So we no longer need the failure return case of this function.
    //
    // Of course, allocations can fail.  Most of the allocating functions in Rust's standard library expect allocations
    // to be infallible, including "Arc" (which was used as the allocator in DataBuffer::new()).  None of these can
    // return an error condition.  But since we call the allocator directly in this implementation, we could now
    // propagate actual allocation failures outward, if we determine that would be helpful.  For now, we stick to the
    // status quo, and assume this allocation never fails.
    pub fn new(capacity: u16) -> Self {
        // Allocate some memory off the heap.
        let (metadata_buf, buffer): (&mut MaybeUninit<MetaData>, &mut [MaybeUninit<u8>]) =
            allocate_metadata_data(capacity);

        Self::new_from_parts(metadata_buf, buffer.as_mut_ptr(), capacity, None)
    }

    /// Create a new buffer using a buffer from the specified [`BufferPool`]. If the pool is empty, this method returns
    /// `None`.
    ///
    /// Note that currently `DemiBuffer` carries static lifetime, so the `BufferPool` must also meet this requirement.
    /// Possibly this requirement could be relaxed with more buffer reference types, or buffers which carry an explicit
    /// lifetime. Until a compelling use case arises, this will cap to `'static`.
    pub fn new_in_pool(pool: &BufferPool) -> Option<Self> {
        let buffer: PoolBuf = match pool.pool().get() {
            Some(buffer) => buffer,
            None => return None,
        };

        let (mut buffer, pool): (NonNull<[MaybeUninit<u8>]>, Rc<MemoryPool>) = PoolBuf::into_raw(buffer);

        // Safety: the buffer size and alignment requirements are enforced by BufferPool.
        let (metadata_buf, buffer): (&mut MaybeUninit<MetaData>, &mut [MaybeUninit<u8>]) =
            unsafe { split_buffer_for_metadata(buffer.as_mut()) };

        assert!(buffer.len() <= (u16::MAX as usize));

        Some(Self::new_from_parts(
            metadata_buf,
            buffer.as_mut_ptr(),
            buffer.len() as u16,
            Some(pool),
        ))
    }

    /// Create a new DemiBuffer in the specified memory, with relevant configuration values.
    fn new_from_parts(
        metadata_buf: &mut MaybeUninit<MetaData>,
        buf_addr: *mut MaybeUninit<u8>,
        capacity: u16,
        pool: Option<Rc<MemoryPool>>,
    ) -> Self {
        let buf_addr: *mut u8 = if capacity > 0 {
            // TODO: casting the MaybeUninit away can cause UB (when deref'd). Change the exposed data type from
            // DemiBuffer to better expose un/initialized values.
            buf_addr.cast()
        } else {
            ptr::null_mut()
        };

        let metadata: NonNull<MetaData> = NonNull::from(metadata_buf.write(MetaData::new(DemiMetaData {
            buf_addr,
            data_off: 0,
            refcnt: 1,
            nb_segs: 1,
            ol_flags: 0,
            pkt_len: capacity as u32,
            // Note: this is not consistent with DPDK behavior: presumably, zero bytes of data are initialized at this
            // point
            data_len: capacity,
            buf_len: capacity,
            next: None,
            pool,
        })));

        // Embed the buffer type into the lower bits of the pointer.
        let tagged: NonNull<MetaData> = metadata.with_addr(metadata.addr() | Tag::Heap);

        // Return the new DemiBuffer.
        DemiBuffer {
            tagged_ptr: tagged,
            _phantom: PhantomData,
        }
    }

    /// Allocate a new DemiBuffer and copy the contents from a byte slice.
    pub fn from_slice(slice: &[u8]) -> Result<Self, Fail> {
        // Note: The implementation of the TryFrom trait (see below, under "Trait Implementations") automatically
        // provides us with a TryInto trait implementation (which is where try_into comes from).
        slice.try_into()
    }

    /// Creates a `DemiBuffer` from a raw pointer.
    pub unsafe fn from_raw(token: NonNull<u8>) -> Self {
        DemiBuffer {
            tagged_ptr: token.cast::<MetaData>(),
            _phantom: PhantomData,
        }
    }

    #[cfg(feature = "libdpdk")]
    /// Creates a `DemiBuffer` from a raw MBuf pointer (*mut rte_mbuf).
    // The MBuf's internal reference count is left unchanged (a reference is effectively donated to the DemiBuffer).
    // Note: Must be called with a non-null (i.e. actual) MBuf pointer.  The MBuf is expected to be in a valid state.
    // It is the caller's responsibility to guarantee this, which is why this function is marked "unsafe".
    pub unsafe fn from_mbuf(mbuf_ptr: *mut rte_mbuf) -> Self {
        // Convert the raw pointer into a NonNull and add a tag indicating it is a DPDK buffer (i.e. a MBuf).
        let temp: NonNull<MetaData> = NonNull::new_unchecked(mbuf_ptr as *mut _);
        let tagged: NonNull<MetaData> = temp.with_addr(temp.addr() | Tag::Dpdk);

        DemiBuffer {
            tagged_ptr: tagged,
            _phantom: PhantomData,
        }
    }

    // ----------------
    // Public Functions
    // ----------------

    /// Returns `true` if this `DemiBuffer` was allocated off of the heap, and `false` otherwise.
    pub fn is_heap_allocated(&self) -> bool {
        self.get_tag() == Tag::Heap
    }

    #[cfg(feature = "libdpdk")]
    /// Returns `true` if this `DemiBuffer` was allocated by DPDK, and `false` otherwise.
    pub fn is_dpdk_allocated(&self) -> bool {
        self.get_tag() == Tag::Dpdk
    }

    /// Returns the length of the data stored in the `DemiBuffer`.
    // Note that while we return a usize here (for convenience), the value is guaranteed to never exceed u16::MAX.
    pub fn len(&self) -> usize {
        self.as_metadata().data_len as usize
    }

    /// Removes `nbytes` bytes from the beginning of the `DemiBuffer` chain.
    // Note: If `nbytes` is greater than the length of the first segment in the chain, then this function will fail and
    // return an error, rather than remove the remaining bytes from subsequent segments in the chain.  This is to match
    // the behavior of DPDK's rte_pktmbuf_adj() routine.
    pub fn adjust(&mut self, nbytes: usize) -> Result<(), Fail> {
        // TODO: Review having this "match", since MetaData and MBuf are laid out the same, these are equivalent cases.
        match self.get_tag() {
            Tag::Heap => {
                let metadata: &mut MetaData = self.as_metadata();
                if nbytes > metadata.data_len as usize {
                    return Err(Fail::new(libc::EINVAL, "tried to remove more bytes than are present"));
                }
                // The above check against data_len also means that nbytes is <= u16::MAX.  So these casts are safe.
                metadata.data_off += nbytes as u16;
                metadata.pkt_len -= nbytes as u32;
                metadata.data_len -= nbytes as u16;
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => {
                let mbuf: *mut rte_mbuf = self.as_mbuf();
                unsafe {
                    // Safety: The `mbuf` dereference below is safe, as it is aligned and dereferenceable.
                    if ((*mbuf).data_len as usize) < nbytes {
                        return Err(Fail::new(libc::EINVAL, "tried to remove more bytes than are present"));
                    }
                }

                // Safety: rte_pktmbuf_adj is a FFI, which is safe since we call it with an actual MBuf pointer.
                if unsafe { rte_pktmbuf_adj(mbuf, nbytes as u16) } == ptr::null_mut() {
                    return Err(Fail::new(libc::EINVAL, "tried to remove more bytes than are present"));
                }
            },
        }

        Ok(())
    }

    /// Removes `nbytes` bytes from the end of the `DemiBuffer` chain.
    // Note: If `nbytes` is greater than the length of the last segment in the chain, then this function will fail and
    // return an error, rather than remove the remaining bytes from subsequent segments in the chain.  This is to match
    // the behavior of DPDK's rte_pktmbuf_trim() routine.
    pub fn trim(&mut self, nbytes: usize) -> Result<(), Fail> {
        // TODO: Review having this "match", since MetaData and MBuf are laid out the same, these are equivalent cases.
        match self.get_tag() {
            Tag::Heap => {
                let md_first: &mut MetaData = self.as_metadata();
                let md_last: &mut MetaData = md_first.get_last_segment();

                if nbytes > md_last.data_len as usize {
                    return Err(Fail::new(libc::EINVAL, "tried to remove more bytes than are present"));
                }
                // The above check against data_len also means that nbytes is <= u16::MAX.  So these casts are safe.
                md_last.data_len -= nbytes as u16;
                md_first.pkt_len -= nbytes as u32;
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => {
                let mbuf: *mut rte_mbuf = self.as_mbuf();
                unsafe {
                    // Safety: The `mbuf` dereference below is safe, as it is aligned and dereferenceable.
                    if ((*mbuf).data_len as usize) < nbytes {
                        return Err(Fail::new(libc::EINVAL, "tried to remove more bytes than are present"));
                    }
                }

                // Safety: rte_pktmbuf_trim is a FFI, which is safe since we call it with an actual MBuf pointer.
                if unsafe { rte_pktmbuf_trim(mbuf, nbytes as u16) } != 0 {
                    return Err(Fail::new(libc::EINVAL, "tried to remove more bytes than are present"));
                }
            },
        }

        Ok(())
    }

    /// Prepends `nbytes` bytes to the begining of the `DemiBuffer`.
    #[cfg(feature = "libdpdk")]
    pub fn prepend(&mut self, nbytes: u16) -> Result<(), Fail> {
        let mbuf: *mut rte_mbuf = unsafe {
            // Safety: rte_pktmbuf_prepend does both sanity and headroom space checks.
            rte_pktmbuf_prepend(self.as_mbuf(), nbytes) as *mut rte_mbuf
        };

        if mbuf.is_null() {
            Err(Fail::new(libc::EINVAL, "tried to prepend more bytes than are allowed"))
        } else {
            Ok(())
        }
    }

    ///
    /// **Description**
    ///
    /// Splits the target [DemiBuffer] at the given `offset` and returns a new [DemiBuffer] containing the data after
    /// the split point (back half).
    ///
    /// The data contained in the new [DemiBuffer] is removed from the original [DemiBuffer] (front half).
    ///
    /// **Return Value**
    ///
    /// On successful completion, a new [DemiBuffer] containing the data after the split point is returned.  On failure,
    /// a [Fail] structure encoding the failure condition is returned instead.
    ///
    /// **Notes**
    ///
    /// - The target [DemiBuffer] must be a single buffer segment (not a chain).
    /// - The target [DemiBuffer] should be large enough to hold `offset`.
    ///
    pub fn split_back(&mut self, offset: usize) -> Result<Self, Fail> {
        self.split(false, offset)
    }

    ///
    /// **Description**
    ///
    /// Splits the target [DemiBuffer] at the given `offset` and returns a new [DemiBuffer] containing the data before
    /// the split point (front half).
    ///
    /// The data contained in the new [DemiBuffer] is removed from the original [DemiBuffer] (back half).
    ///
    /// **Return Value**
    ///
    /// On successful completion, a new [DemiBuffer] containing the data before the split point is returned. On failure,
    /// a [Fail] structure encoding the failure condition is returned instead.
    ///
    /// **Notes**
    ///
    /// - The target [DemiBuffer] must be a single buffer segment (not a chain).
    /// - The target [DemiBuffer] should be large enough to hold `offset`.
    ///
    pub fn split_front(&mut self, offset: usize) -> Result<Self, Fail> {
        self.split(true, offset)
    }

    ///
    /// **Description**
    ///
    /// Splits the target [DemiBuffer] at the given `offset` and returns a new [DemiBuffer] containing the data removed
    /// from the original buffer.
    ///
    /// **Return Value**
    ///
    /// On successful completion, a new [DemiBuffer] containing the data removed from the original buffer is returned.
    /// On failure, a [Fail] structure encoding the failure condition is returned instead.
    ///
    fn split(&mut self, split_front: bool, offset: usize) -> Result<Self, Fail> {
        // Check if this is a multi-segment buffer.
        if self.is_multi_segment() {
            let cause: String = format!("cannot split a multi-segment buffer");
            error!("split_front(): {}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // Check if split offset is valid.
        if self.len() < offset {
            let cause: String = format!("cannot split buffer at given offset (offset={:?})", offset);
            error!("split_front(): {}", &cause);
            return Err(Fail::new(libc::EINVAL, &cause));
        }

        // Clone the target buffer before any changes are applied.
        let mut cloned_buf: DemiBuffer = self.clone();

        if split_front {
            // Remove data starting at `offset` from the front half buffer (cloned buffer).
            // Those bytes now belong to the back half buffer.
            // This unwrap won't panic as we already performed its error checking above.
            cloned_buf.trim(self.len() - offset).unwrap();

            // Remove `offset` bytes from the beginning of the back half buffer (original buffer).
            // Those bytes now belong to the front back buffer.
            // This unwrap won't panic as we already performed its error checking above.
            self.adjust(offset).unwrap();
        } else {
            // Remove data starting at `offset` from the front half buffer (original buffer).
            // Those bytes now belong to the back half buffer.
            // This unwrap won't panic as we already performed its error checking above.
            self.trim(self.len() - offset).unwrap();

            // Remove `offset` bytes from the beginning of the back half buffer (cloned buffer).
            // Those bytes now belong to the front back buffer.
            // This unwrap won't panic as we already performed its error checking above.
            cloned_buf.adjust(offset).unwrap();
        }

        // Return the cloned buffer.
        Ok(cloned_buf)
    }

    /// Provides a raw pointer to the buffer data.
    ///
    /// The reference count is not affected in any way and the DemiBuffer is not consumed.  The pointer is valid for as
    /// long as there is a DemiBuffer in existance that is holding a reference on this data.
    // This function is not marked unsafe, as the unsafe act is dereferencing the returned pointer, not providing it.
    pub fn as_ptr(&self) -> *const u8 {
        // TODO: Review having this "match", since MetaData and MBuf are laid out the same, these are equivalent cases.
        match self.get_tag() {
            Tag::Heap => self.data_ptr(),
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => self.dpdk_data_ptr(),
        }
    }

    /// Consumes the `DemiBuffer`, returning a raw token (useful for FFI) that can be used with `from_raw()`.
    // Note the type of the token is arbitrary, it should be treated as an opaque value.
    pub fn into_raw(self) -> NonNull<u8> {
        let raw_token = self.tagged_ptr.cast::<u8>();
        // Don't run the DemiBuffer destructor on this.
        mem::forget(self);
        raw_token
    }

    /// Consumes the `DemiBuffer`, returning the contained MBuf pointer.
    // The returned MBuf takes all existing references on the data with it (the DemiBuffer donates its ref to the MBuf).
    #[cfg(feature = "libdpdk")]
    pub fn into_mbuf(self) -> Option<*mut rte_mbuf> {
        if self.get_tag() == Tag::Dpdk {
            let mbuf = self.as_mbuf();
            // Don't run the DemiBuffer destructor on this.
            mem::forget(self);
            Some(mbuf)
        } else {
            None
        }
    }

    // ------------------
    // Internal Functions
    // ------------------

    // Gets the tag containing the type of DemiBuffer.
    #[inline]
    fn get_tag(&self) -> Tag {
        Tag::from(usize::from(self.tagged_ptr.addr()) & Tag::MASK)
    }

    // Gets the untagged pointer to the underlying type.
    #[inline]
    fn get_ptr<U>(&self) -> NonNull<U> {
        // Safety: The call to NonZeroUsize::new_unchecked is safe, as its argument is guaranteed to be non-zero.
        let address: NonZeroUsize =
            unsafe { NonZeroUsize::new_unchecked(usize::from(self.tagged_ptr.addr()) & !Tag::MASK) };
        self.tagged_ptr.with_addr(address).cast::<U>()
    }

    // Gets the DemiBuffer as a mutable MetaData reference.
    // Note: Caller is responsible for enforcing Rust's aliasing rules for the returned MetaData reference.
    #[inline]
    fn as_metadata(&self) -> &mut MetaData {
        // Safety: The call to as_mut is safe, as the pointer is aligned and dereferenceable, and the MetaData struct
        // it points to is initialized properly.
        unsafe { self.get_ptr::<MetaData>().as_mut() }
    }

    // Gets the DemiBuffer as a mutable MBuf pointer.
    #[cfg(feature = "libdpdk")]
    #[inline]
    fn as_mbuf(&self) -> *mut rte_mbuf {
        self.get_ptr::<rte_mbuf>().as_ptr()
    }

    // Gets a raw pointer to the DemiBuffer data.
    fn data_ptr(&self) -> *mut u8 {
        let metadata: &mut MetaData = self.as_metadata();
        let buf_ptr: *mut u8 = metadata.buf_addr;
        // Safety: The call to offset is safe, as its argument is known to remain within the allocated region.
        unsafe { buf_ptr.offset(metadata.data_off as isize) }
    }

    // Gets a raw pointer to the DemiBuffer data (DPDK type specific).
    // Note: Since our MetaData and DPDK's rte_mbuf have equivalent layouts for the buf_addr and data_off fields, this
    // function isn't strictly necessary, as it does the exact same thing as data_ptr() does.
    #[cfg(feature = "libdpdk")]
    fn dpdk_data_ptr(&self) -> *mut u8 {
        let mbuf: *mut rte_mbuf = self.as_mbuf();
        unsafe {
            // Safety: It is safe to dereference "mbuf" as it is known to be valid.
            let buf_ptr: *mut u8 = (*mbuf).buf_addr as *mut u8;
            // Safety: The call to offset is safe, as its argument is known to remain within the allocated region.
            buf_ptr.offset((*mbuf).data_off as isize)
        }
    }

    ///
    /// **Description**
    ///
    /// Checks if the target [DemiBuffer] has multiple segments or not.
    ///
    /// **Return Value**
    ///
    /// If the target [DemiBuffer] has multiple segments, `true` is returned. Otherwise, `false` is returned instead.
    ///
    fn is_multi_segment(&self) -> bool {
        match self.get_tag() {
            Tag::Heap => {
                let md_front: &MetaData = self.as_metadata();
                md_front.nb_segs != 1
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => {
                let mbuf: *const rte_mbuf = self.as_mbuf();
                // Safety: The `mbuf` dereferences in this block are safe, as it is aligned and dereferenceable.
                unsafe { (*mbuf).nb_segs != 1 }
            },
        }
    }
}

// ----------------
// Helper Functions
// ----------------

// Allocates the MetaData (plus the space for any directly attached data) for a new heap-allocated DemiBuffer.
fn allocate_metadata_data<'a>(direct_data_size: u16) -> (&'a mut MaybeUninit<MetaData>, &'a mut [MaybeUninit<u8>]) {
    // We need space for the MetaData struct, plus any extra memory for directly attached data.
    let amount: usize = size_of::<MetaData>() + direct_data_size as usize;

    // Given our limited allocation amount (u16::MAX) and fixed alignment size, this unwrap cannot panic.
    let layout: Layout = Layout::from_size_align(amount, arch::CPU_DATA_CACHE_LINE_SIZE).unwrap();

    // Safety: This is safe, as we check for a null return value before dereferencing "allocation".
    let allocation: *mut MaybeUninit<u8> = unsafe { alloc(layout) }.cast();
    if allocation.is_null() {
        handle_alloc_error(layout);
    }

    // Safety: the slice is valid based on the constraints to the above allocation.
    let buffer: &mut [MaybeUninit<u8>] = unsafe { slice::from_raw_parts_mut(allocation, amount) };

    // Safety: buffer is aligned to CPU_DATA_CACHE_LINE_SIZE (which is overaligned for MetaData) and will always be no
    // smaller than MetaData.
    unsafe { split_buffer_for_metadata(buffer) }
}

/// Split a buffer into (metadata, data) parts.
///
/// # Panics:
/// panics if `buffer.len() < size_of::<MetaData>()`.
///
/// # Safety:
/// `buffer` must be suitably aligned for and large enough to hold a [`MetaData`].
unsafe fn split_buffer_for_metadata<'a>(
    buffer: &'a mut [MaybeUninit<u8>],
) -> (&'a mut MaybeUninit<MetaData>, &'a mut [MaybeUninit<u8>]) {
    assert!(buffer.len() >= size_of::<MetaData>());

    let (metadata_buf, data_buf) = buffer.split_at_mut(size_of::<MetaData>());

    // Safety: buffer is not null and properly aligned since it comes from a reference. MaybeUninit does not
    // require initialization.
    let metadata: &mut MaybeUninit<MetaData> =
        unsafe { &mut *metadata_buf.as_mut_ptr().cast::<MaybeUninit<MetaData>>() };

    (metadata, data_buf)
}

// Frees the MetaData (plus the space for any directly attached data) for a heap-allocated DemiBuffer.
fn free_metadata_data(mut buffer: NonNull<MetaData>) {
    let (amount, pool): (usize, Option<Rc<MemoryPool>>) = {
        // Safety: This is safe, as `buffer` is aligned, dereferenceable, and we don't let `metadata` escape this function.
        let metadata: &mut MetaData = unsafe { buffer.as_mut() };

        // Determine the size of the original allocation.
        // Note that this code currently assumes we're not using a "private data" feature akin to DPDK's.
        // Safety: _priv_size will be initialized when debug_assertions is turned on.
        debug_assert_eq!(unsafe { metadata._priv_size.assume_init() }, 0);

        (size_of::<MetaData>() + metadata.buf_len as usize, metadata.pool.take())
    };

    // Drop the instance.
    // Safety: the pointer `buffer` is valid, aligned, and properly initialized.
    unsafe { ptr::drop_in_place(buffer.as_ptr()) };

    // This unwrap will never panic, as we pass a known allocation amount and a fixed alignment to from_size_align().
    match pool {
        Some(pool) => {
            // Safety: the pool pointer is populated from a 'static reference, so will be valid and dereferenceable.
            // Because the MetaData buffer must come from `pool`, the slice with size `pool.layout()` starting at
            // `buffer` will also be valid and dereferenceable. The `MetaData` buffer is created from
            // `PoolBuf::into_raw` by the constructor, so the buffer may be passed back to `PoolBuf::from_raw`.
            unsafe {
                let pool_layout: Layout = pool.layout();
                let mem_slice: &mut [MaybeUninit<u8>] =
                    slice::from_raw_parts_mut(buffer.cast::<MaybeUninit<u8>>().as_ptr(), pool_layout.size());
                mem::drop(PoolBuf::from_raw(NonNull::from(mem_slice), pool));
            }
        },

        None => {
            // Convert buffer pointer into a raw allocation pointer.
            let allocation: *mut u8 = buffer.cast::<u8>().as_ptr();
            let layout: Layout = Layout::from_size_align(amount, arch::CPU_DATA_CACHE_LINE_SIZE).unwrap();

            // Safety: this is safe because we're using the same (de)allocator and Layout used for allocation.
            unsafe { dealloc(allocation, layout) };
        },
    }
}

// ---------------------
// Trait Implementations
// ---------------------

/// Clone Trait Implementation for `DemiBuffer`.
impl Clone for DemiBuffer {
    fn clone(&self) -> Self {
        match self.get_tag() {
            Tag::Heap => {
                // To create a clone (not a copy), we construct a new indirect buffer for each buffer segment in the
                // original buffer chain.  An indirect buffer has its own MetaData struct representing its view into
                // the data, but the data itself resides in the original direct buffer and isn't copied.  Instead,
                // we increment the reference count on that data.

                // Allocate space for a new MetaData struct without any direct data.  This will become the clone.
                // TODO: Pooled MetaData should be reallocated from the pool.
                let (head, _): (&mut MaybeUninit<MetaData>, _) = allocate_metadata_data(0);
                let mut temp: NonNull<MaybeUninit<MetaData>> = NonNull::from(&*head);

                // This might be a chain of buffers.  If so, we'll walk the list.  There is always a first one.
                let mut next_entry: Option<NonNull<MetaData>> = Some(self.get_ptr::<MetaData>());
                while let Some(mut entry) = next_entry {
                    // Safety: This is safe, as `entry` is aligned, dereferenceable, and the MetaData struct it
                    // points to is initialized.
                    let original: &mut MetaData = unsafe { entry.as_mut() };

                    // Remember the next entry in the chain.
                    next_entry = original.next;

                    // Initialize the MetaData of the indirect buffer.
                    {
                        // Safety: Safe, as `temp` is aligned, dereferenceable, and `clone` isn't aliased in this block.
                        let clone: &mut MaybeUninit<MetaData> = unsafe { temp.as_mut() };

                        // Next needs to point to the next entry in the cloned chain, not the original.
                        let next: Option<NonNull<MetaData>> = if next_entry.is_none() {
                            None
                        } else {
                            // Allocate space for the next segment's MetaData struct.
                            let (new_metadata, _) = allocate_metadata_data(0);
                            temp = NonNull::from(new_metadata);
                            Some(temp.cast())
                        };

                        // Add indirect flag to clone for non-empty buffers. Empty buffers don't reference any data, so
                        // aren't indirect.
                        let ol_flags: u64 =
                            original.ol_flags | if original.buf_len != 0 { METADATA_F_INDIRECT } else { 0 };

                        // Copy other relevant fields from our progenitor.
                        let values: DemiMetaData = DemiMetaData {
                            // Our cloned segment has only one reference (the one we return from this function).
                            refcnt: 1,
                            next,
                            buf_addr: original.buf_addr,
                            buf_len: original.buf_len,
                            data_off: original.data_off,
                            nb_segs: original.nb_segs,
                            pkt_len: original.pkt_len,
                            data_len: original.data_len,
                            ol_flags,
                            pool: None,
                        };

                        clone.write(MetaData::new(values));

                        // Special case for zero-length buffers.
                        if original.buf_len == 0 {
                            debug_assert_eq!(original.buf_addr, ptr::null_mut());
                            // Since there is no data to clone, we don't need to increment any reference counts.
                            // Instead we just create a new zero-length direct buffer.
                            continue;
                        }
                    }

                    // Increment the reference count on the data.  It resides in the MetaData structure that the data
                    // is directly attached to.  If the buffer we're cloning is itself an indirect buffer, then we need
                    // to find the original direct buffer in order to increment the correct reference count.
                    if original.ol_flags & METADATA_F_INDIRECT == 0 {
                        // Cloning a direct buffer.  Increment the ref count on it.
                        original.inc_refcnt();
                    } else {
                        // Cloning an indirect buffer.  Increment the ref count on the direct buffer with the data.
                        // The direct buffer's MetaData struct should immediately preceed the actual data.
                        let offset: isize = -(size_of::<MetaData>() as isize);
                        let direct: &mut MetaData = unsafe {
                            // Safety: The offset call is safe as `offset` is known to be "in bounds" for buf_addr.
                            // Safety: The as_mut call is safe as the pointer is aligned, dereferenceable, and
                            // points to an initialized MetaData instance.
                            // The returned address is known to be non-Null, so the unwrap call will never panic.
                            original.buf_addr.offset(offset).cast::<MetaData>().as_mut().unwrap()
                        };
                        direct.inc_refcnt();
                    }
                }

                // Embed the buffer type into the lower bits of the pointer.
                // Safety: head is initialized by the above loop.
                let head_ptr: NonNull<MetaData> = NonNull::from(unsafe { head.assume_init_mut() });
                let tagged: NonNull<MetaData> = head_ptr.with_addr(head_ptr.addr() | Tag::Heap);

                // Return the new DemiBuffer.
                DemiBuffer {
                    tagged_ptr: tagged,
                    _phantom: PhantomData,
                }
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => unsafe {
                let mbuf_ptr: *mut rte_mbuf = self.as_mbuf();
                // TODO: This allocates the clone MBuf from the same MBuf pool as the original MBuf.  Since the clone
                // never has any direct data, we could potentially save memory by allocating these from a special pool.
                // Safety: it is safe to dereference "mbuf_ptr" as it is known to point to a valid MBuf.
                let mempool_ptr: *mut rte_mempool = (*mbuf_ptr).pool;
                // Safety: rte_pktmbuf_clone is a FFI, which is safe to call since we call it with valid arguments and
                // properly check its return value for null (failure) before using.
                let mbuf_ptr_clone: *mut rte_mbuf = rte_pktmbuf_clone(mbuf_ptr, mempool_ptr);
                if mbuf_ptr_clone.is_null() {
                    panic!("failed to clone mbuf");
                }

                // Safety: from_mbuf is safe to call here as "mbuf_ptr_clone" is known to point to a valid MBuf.
                DemiBuffer::from_mbuf(mbuf_ptr_clone)
            },
        }
    }
}

/// De-Reference Trait Implementation for `DemiBuffer`.
impl Deref for DemiBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // TODO: Review having this "match", since MetaData and MBuf are laid out the same, these are equivalent cases.
        match self.get_tag() {
            Tag::Heap => {
                // If the buffer is empty, return an empty slice.
                if self.len() == 0 {
                    return &[];
                }

                // Safety: the call to from_raw_parts is safe, as its arguments refer to a valid readable memory region
                // of the size specified (which is guaranteed to be smaller than isize::MAX) and is contained within
                // a single allocated object.  Also, since the data type is u8, proper alignment is not an issue.
                unsafe { slice::from_raw_parts(self.data_ptr(), self.len()) }
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => {
                // Safety: the call to from_raw_parts is safe, as its arguments refer to a valid readable memory region
                // of the size specified (which is guaranteed to be smaller than isize::MAX) and is contained within
                // a single allocated object.  Also, since the data type is u8, proper alignment is not an issue.
                unsafe { slice::from_raw_parts(self.dpdk_data_ptr(), self.len()) }
            },
        }
    }
}

/// Mutable De-Reference Trait Implementation for `DemiBuffer`.
impl DerefMut for DemiBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        // TODO: Review having this "match", since MetaData and MBuf are laid out the same, these are equivalent cases.
        match self.get_tag() {
            Tag::Heap => {
                // Safety: the call to from_raw_parts_mut is safe, as its args refer to a valid readable memory region
                // of the size specified (which is guaranteed to be smaller than isize::MAX) and is contained within
                // a single allocated object.  Also, since the data type is u8, proper alignment is not an issue.
                unsafe { slice::from_raw_parts_mut(self.data_ptr(), self.len()) }
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => {
                // Safety: the call to from_raw_parts_mut is safe, as its args refer to a valid readable memory region
                // of the size specified (which is guaranteed to be smaller than isize::MAX) and is contained within
                // a single allocated object.  Also, since the data type is u8, proper alignment is not an issue.
                unsafe { slice::from_raw_parts_mut(self.dpdk_data_ptr(), self.len()) }
            },
        }
    }
}

/// Drop Trait Implementation for `DemiBuffer`.
impl Drop for DemiBuffer {
    fn drop(&mut self) {
        match self.get_tag() {
            Tag::Heap => {
                // This might be a chain of buffers.  If so, we'll walk the list.
                let mut next_entry: Option<NonNull<MetaData>> = Some(self.get_ptr());
                while let Some(mut entry) = next_entry {
                    // Safety: This is safe, as `entry` is aligned, dereferenceable, and the MetaData struct it points
                    // to is initialized.
                    let metadata: &mut MetaData = unsafe { entry.as_mut() };

                    // Remember the next entry in the chain (if any) before we potentially free the current one.
                    next_entry = metadata.next;
                    metadata.next = None;
                    metadata.nb_segs = 1;

                    // Decrement the reference count.
                    if metadata.dec_refcnt() == 0 {
                        // See if the data is directly attached, or indirectly attached.
                        if metadata.ol_flags & METADATA_F_INDIRECT != 0 {
                            // This is an indirect buffer.  Find the direct buffer that holds the actual data.
                            let offset: isize = -(size_of::<MetaData>() as isize);
                            let direct: &mut MetaData = unsafe {
                                // Safety: The offset call is safe as `offset` is known to be "in bounds" for buf_addr.
                                // Safety: The as_mut call is safe as the pointer is aligned, dereferenceable, and
                                // points to an initialized MetaData instance.
                                // The returned address is known to be non-Null, so the unwrap call will never panic.
                                metadata.buf_addr.offset(offset).cast::<MetaData>().as_mut().unwrap()
                            };

                            // Restore buf_addr and buf_len to their unattached values.
                            metadata.buf_addr = null_mut();
                            metadata.buf_len = 0;
                            metadata.ol_flags = metadata.ol_flags & !METADATA_F_INDIRECT;

                            // Drop our reference to the direct buffer, and free it if ours was the last one.
                            if direct.dec_refcnt() == 0 {
                                // Verify this is a direct buffer in debug builds.
                                debug_assert_eq!(direct.ol_flags & METADATA_F_INDIRECT, 0);

                                // Convert to NonNull<MetaData> type.
                                // Safety: The NonNull::new_unchecked call is safe, as `direct` is known to be non-null.
                                let allocation: NonNull<MetaData> = unsafe { NonNull::new_unchecked(direct as *mut _) };

                                // Free the direct buffer.
                                free_metadata_data(allocation);
                            }
                        }

                        // Free this buffer.
                        free_metadata_data(entry);
                    }
                }
            },
            #[cfg(feature = "libdpdk")]
            Tag::Dpdk => {
                let mbuf_ptr: *mut rte_mbuf = self.as_mbuf();
                // Safety: This is safe, as mbuf_ptr does indeed point to a valid MBuf.
                unsafe {
                    // Note: This DPDK routine properly handles MBuf chains, as well as indirect, and external MBufs.
                    rte_pktmbuf_free(mbuf_ptr);
                }
            },
        }
    }
}

/// TryFrom Trait Implementation for `DemiBuffer`.
impl TryFrom<&[u8]> for DemiBuffer {
    type Error = Fail;

    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        // Check size of the slice to ensure a single DemiBuffer can hold it.
        let size: u16 = if slice.len() < u16::MAX as usize {
            slice.len() as u16
        } else {
            return Err(Fail::new(libc::EINVAL, "slice is larger than a DemiBuffer can hold"));
        };

        // Allocate some memory off the heap.
        let (temp, buffer): (&mut MaybeUninit<MetaData>, &mut [MaybeUninit<u8>]) = allocate_metadata_data(size);

        // Point buf_addr at the newly allocated data space (if any).
        let buf_addr: *mut u8 = if size == 0 {
            // No direct data, so don't point buf_addr at anything.
            null_mut()
        } else {
            let buf_addr: *mut u8 = buffer.as_mut_ptr().cast();

            // Copy the data from the slice into the DemiBuffer.
            // Safety: This is safe, as the src/dst argument pointers are valid for reads/writes of `size` bytes,
            // are aligned (trivial for u8 pointers), and the regions they specify do not overlap one another.
            unsafe { ptr::copy_nonoverlapping(slice.as_ptr(), buf_addr, size as usize) };
            buf_addr
        };

        // Set field values as appropriate.
        let metadata: NonNull<MetaData> = NonNull::from(temp.write(MetaData::new(DemiMetaData {
            buf_addr,
            data_off: 0,
            refcnt: 1,
            nb_segs: 1,
            ol_flags: 0,
            pkt_len: size as u32,
            data_len: size,
            buf_len: size,
            next: None,
            pool: None,
        })));

        // Embed the buffer type into the lower bits of the pointer.
        let tagged: NonNull<MetaData> = metadata.with_addr(metadata.addr() | Tag::Heap);

        // Return the new DemiBuffer.
        Ok(DemiBuffer {
            tagged_ptr: tagged,
            _phantom: PhantomData,
        })
    }
}

// Unit tests for `DemiBuffer` type.
// Note that due to DPDK being a configurable option, all of these unit tests are only for heap-allocated `DemiBuffer`s.
#[cfg(test)]
mod tests {
    use crate::runtime::memory::demibuffer::DemiBuffer;
    use ::anyhow::Result;
    use std::ptr::NonNull;

    // Test basic allocation, len, adjust, and trim.
    #[test]
    fn basic() -> Result<()> {
        // Create a new (heap-allocated) `DemiBuffer` with a 42 byte data area.
        let mut buf: DemiBuffer = DemiBuffer::new(42);
        crate::ensure_eq!(buf.is_heap_allocated(), true);
        crate::ensure_eq!(buf.len(), 42);

        // Remove 7 bytes from the beginning of the data area.  Length should now be 35.
        crate::ensure_eq!(buf.adjust(7).is_ok(), true);
        crate::ensure_eq!(buf.len(), 35);

        // Remove 7 bytes from the end of the data area.  Length should now be 28.
        crate::ensure_eq!(buf.trim(7).is_ok(), true);
        crate::ensure_eq!(buf.len(), 28);

        // Verify bad requests actually fail.
        crate::ensure_eq!(buf.adjust(30).is_err(), true);
        crate::ensure_eq!(buf.trim(30).is_err(), true);

        Ok(())
    }

    // Test cloning, raw conversion, and zero-size buffers.
    #[test]
    fn advanced() -> Result<()> {
        fn clone_me(buf: DemiBuffer) -> DemiBuffer {
            // Clone and return the buffer.
            buf.clone()
            // `buf` should be dropped here.
        }

        fn convert_to_token(buf: DemiBuffer) -> NonNull<u8> {
            // Convert the buffer into a raw token.
            buf.into_raw()
            // `buf` was consumed by into_raw(), so it isn't dropped here.  The token holds the reference.
        }

        // Create a buffer and clone it.
        let fortytwo: DemiBuffer = DemiBuffer::new(42);
        crate::ensure_eq!(fortytwo.len(), 42);
        let clone: DemiBuffer = clone_me(fortytwo);
        crate::ensure_eq!(clone.len(), 42);

        // Convert a buffer into a raw token and bring it back.
        let token: NonNull<u8> = convert_to_token(clone);
        let reconstituted: DemiBuffer = unsafe { DemiBuffer::from_raw(token) };
        crate::ensure_eq!(reconstituted.len(), 42);

        // Create a zero-sized buffer.
        let zero: DemiBuffer = DemiBuffer::new(0);
        crate::ensure_eq!(zero.len(), 0);
        // Clone it, and keep the original around.
        let clone: DemiBuffer = zero.clone();
        crate::ensure_eq!(clone.len(), 0);
        // Clone the clone, and drop the first clone.
        let another: DemiBuffer = clone_me(clone);
        crate::ensure_eq!(another.len(), 0);

        Ok(())
    }

    // Tests split_back (and also allocation from a slice).
    #[test]
    fn split_back() -> Result<()> {
        // Create a new (heap-allocated) `DemiBuffer` by copying a slice of a `String`.
        let str: String = String::from("word one two three four five six seven eight nine");
        let slice: &[u8] = str.as_bytes();
        // `DemiBuffer::from_slice` shouldn't fail, as we passed it a valid slice of a `DemiBuffer`-allowable length.
        let mut buf: DemiBuffer = match DemiBuffer::from_slice(slice) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!(
                "DemiBuffer::from_slice should return a DemiBuffer for this slice: {}",
                e
            ),
        };

        // The `DemiBuffer` data length should equal the original string length.
        crate::ensure_eq!(buf.len(), str.len());

        // The `DemiBuffer` data (content) should match that of the original string.
        crate::ensure_eq!(&*buf, slice);

        // Split this `DemiBuffer` into two.
        // `DemiBuffer::split_back` shouldn't fail, as we passed it a valid offset.
        let mut split_buf: DemiBuffer = match buf.split_back(24) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!("DemiBuffer::split_back shouldn't fail for this offset: {}", e),
        };
        crate::ensure_eq!(buf.len(), 24);
        crate::ensure_eq!(split_buf.len(), 25);

        // Compare contents.
        crate::ensure_eq!(&buf[..], &str.as_bytes()[..24]);
        crate::ensure_eq!(&split_buf[..], &str.as_bytes()[24..]);

        // Split another `DemiBuffer` off of the already-split-off one.
        // `DemiBuffer::split_back` shouldn't fail, as we passed it a valid offset.
        let another_buf: DemiBuffer = match split_buf.split_back(9) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!("DemiBuffer::split_back shouldn't fail for this offset: {}", e),
        };

        crate::ensure_eq!(buf.len(), 24);
        crate::ensure_eq!(split_buf.len(), 9);
        crate::ensure_eq!(another_buf.len(), 16);

        // Compare contents (including the unaffected original to ensure that it is actually unaffected).
        crate::ensure_eq!(&buf[..], &str.as_bytes()[..24]);
        crate::ensure_eq!(&split_buf[..], &str.as_bytes()[24..33]);
        crate::ensure_eq!(&another_buf[..], &str.as_bytes()[33..]);

        Ok(())
    }

    // Test split_off (and also allocation from a slice).
    #[test]
    fn split_front() -> Result<()> {
        // Create a new (heap-allocated) `DemiBuffer` by copying a slice of a `String`.
        let str: String = String::from("word one two three four five six seven eight nine");
        let slice: &[u8] = str.as_bytes();
        // `DemiBuffer::from_slice` shouldn't fail, as we passed it a valid slice of a `DemiBuffer`-allowable length.
        let mut buf: DemiBuffer = match DemiBuffer::from_slice(slice) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!(
                "DemiBuffer::from_slice should return a DemiBuffer for this slice: {}",
                e
            ),
        };

        // The `DemiBuffer` data length should equal the original string length.
        crate::ensure_eq!(buf.len(), str.len());

        // The `DemiBuffer` data (content) should match that of the original string.
        crate::ensure_eq!(&*buf, slice);

        // Split this `DemiBuffer` into two.
        // `DemiBuffer::split_off` shouldn't fail, as we passed it a valid offset.
        let mut split_buf: DemiBuffer = match buf.split_front(24) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!("DemiBuffer::split_off shouldn't fail for this offset: {}", e),
        };
        crate::ensure_eq!(buf.len(), 25);
        crate::ensure_eq!(split_buf.len(), 24);

        // Compare contents.
        crate::ensure_eq!(&buf[..], &str.as_bytes()[24..]);
        crate::ensure_eq!(&split_buf[..], &str.as_bytes()[..24]);

        // Split another `DemiBuffer` off of the already-split-off one.
        // `DemiBuffer::split_off` shouldn't fail, as we passed it a valid offset.
        let another_buf: DemiBuffer = match split_buf.split_front(9) {
            Ok(buf) => buf,
            Err(e) => anyhow::bail!("DemiBuffer::split_off shouldn't fail for this offset: {}", e),
        };
        crate::ensure_eq!(buf.len(), 25);
        crate::ensure_eq!(split_buf.len(), 15);
        crate::ensure_eq!(another_buf.len(), 9);

        // Compare contents (including the unaffected original to ensure that it is actually unaffected).
        crate::ensure_eq!(&buf[..], &str.as_bytes()[24..]);
        crate::ensure_eq!(&split_buf[..], &str.as_bytes()[9..24]);
        crate::ensure_eq!(&another_buf[..], &str.as_bytes()[..9]);

        Ok(())
    }
}
