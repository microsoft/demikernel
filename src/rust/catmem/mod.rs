// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::futures::{
    pop::PopFuture,
    push::PushFuture,
    Operation,
    OperationResult,
};
use crate::{
    collections::shared_ring::SharedRingBuffer,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        queue::IoQueueTable,
        types::{
            demi_opcode_t,
            demi_qr_value_t,
            demi_qresult_t,
            demi_sgarray_t,
            demi_sgaseg_t,
        },
        QDesc,
        QToken,
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
    },
    QType,
};
use ::std::{
    any::Any,
    collections::HashMap,
    mem,
    ptr,
    ptr::NonNull,
    rc::Rc,
};

//======================================================================================================================
// Constants
//======================================================================================================================

const RING_BUFFER_CAPACITY: usize = 4096;

//======================================================================================================================
// Structures
//======================================================================================================================

/// A LibOS that exposes a memory queue.
pub struct CatmemLibOS {
    qtable: IoQueueTable,
    scheduler: Scheduler,
    rings: HashMap<QDesc, Rc<SharedRingBuffer<u8>>>,
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for Catmem LibOS.
impl CatmemLibOS {
    /// Instantiates a new LibOS.
    pub fn new() -> Self {
        CatmemLibOS {
            qtable: IoQueueTable::new(),
            scheduler: Scheduler::default(),
            rings: HashMap::new(),
        }
    }

    /// Creates a new memory queue.
    pub fn create_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("create_pipe() name={:?}", name);

        let ring: SharedRingBuffer<u8> = SharedRingBuffer::<u8>::create(name, RING_BUFFER_CAPACITY)?;

        let qd: QDesc = self.qtable.alloc(QType::MemoryQueue.into());
        assert_eq!(self.rings.insert(qd, Rc::new(ring)).is_none(), true);

        Ok(qd)
    }

    /// Opens a memory queue.
    pub fn open_pipe(&mut self, name: &str) -> Result<QDesc, Fail> {
        trace!("open_pipe() name={:?}", name);

        let ring: SharedRingBuffer<u8> = SharedRingBuffer::<u8>::open(name, RING_BUFFER_CAPACITY)?;

        let qd: QDesc = self.qtable.alloc(QType::MemoryQueue.into());
        assert_eq!(self.rings.insert(qd, Rc::new(ring)).is_none(), true);

        Ok(qd)
    }

    /// Closes a memory queue.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        match self.rings.remove(&qd) {
            Some(_) => {
                self.qtable.free(qd);
                Ok(())
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        match self.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue push operation.
                match self.rings.get(&qd) {
                    Some(ring) => {
                        let future: Operation = Operation::from(PushFuture::new(qd, ring.clone(), buf));
                        let handle: SchedulerHandle = match self.scheduler.insert(future) {
                            Some(handle) => handle,
                            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                        };
                        let qt: QToken = handle.into_raw().into();
                        trace!("push() qt={:?}", qt);
                        Ok(qt)
                    },
                    _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
                }
            },
            Err(e) => Err(e),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        // Issue pop operation.
        match self.rings.get(&qd) {
            Some(ring) => {
                let future: Operation = Operation::from(PopFuture::new(qd, ring.clone()));
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                trace!("pop() qt={:?}", qt);
                Ok(qt)
            },
            None => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn alloc_sgarray(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        // ToDo: Allocate an array of buffers if requested size is too large for a single buffer.

        // We can't allocate more than a single buffer.
        if size > u16::MAX as usize {
            return Err(Fail::new(libc::EINVAL, "size too large for a single demi_sgaseg_t"));
        }

        // First allocate the underlying (heap-allocated) DemiBuffer.
        let buf: DemiBuffer = DemiBuffer::new(size as u16);

        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut libc::c_void,
            sgaseg_len: size as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut libc::c_void,
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
    fn clone_sgarray(&self, sga: &demi_sgarray_t) -> Result<DemiBuffer, Fail> {
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

    /// Takes out the [OperationResult] associated with the target [SchedulerHandle].
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.scheduler.take(handle).as_any();
        let boxed_concrete_type: Operation = *boxed_future.downcast::<Operation>().expect("Wrong type!");

        boxed_concrete_type.get_result()
    }

    /// Converts a runtime buffer into a scatter-gather array.
    fn into_sgarray(buf: DemiBuffer) -> Result<demi_sgarray_t, Fail> {
        // Create a scatter-gather segment to expose the DemiBuffer to the user.
        let data: *const u8 = buf.as_ptr();
        let sga_seg: demi_sgaseg_t = demi_sgaseg_t {
            sgaseg_buf: data as *mut libc::c_void,
            sgaseg_len: buf.len() as u32,
        };

        // Create and return a new scatter-gather array (which inherits the DemiBuffer's reference).
        Ok(demi_sgarray_t {
            sga_buf: buf.into_raw().as_ptr() as *mut libc::c_void,
            sga_numsegs: 1,
            sga_segs: [sga_seg],
            sga_addr: unsafe { mem::zeroed() },
        })
    }

    pub fn schedule(&mut self, qt: QToken) -> Result<SchedulerHandle, Fail> {
        match self.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => Ok(handle),
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        }
    }

    pub fn pack_result(&mut self, handle: SchedulerHandle, qt: QToken) -> Result<demi_qresult_t, Fail> {
        let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
        Ok(pack_result(r, qd, qt.into()))
    }

    pub fn poll(&self) {
        self.scheduler.poll()
    }
}

//======================================================================================================================
// Standalone Functions
//======================================================================================================================

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Push => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(bytes) => match CatmemLibOS::into_sgarray(bytes) {
            Ok(sga) => {
                let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}
